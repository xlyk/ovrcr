//! Current-Dashboard model discovery and validation for managed agent launches.
use super::*;
use anyhow::Context;
use ovrcr_protocol::{
    AgentLaunchOrigin, AgentModelCapabilities, AgentModelOption, AgentModelPolicy,
    CreateAgentSessionRequest, CreateSessionRequest, ErrorCode,
};
use sha2::{Digest, Sha256};
use std::os::unix::{ffi::OsStrExt, fs::MetadataExt};
use std::sync::atomic::AtomicBool;
use std::sync::{Condvar, MutexGuard, TryLockError};

#[derive(Clone)]
struct QueryInput {
    preset: String,
    origin: AgentLaunchOrigin,
    argv: Vec<std::ffi::OsString>,
    project: String,
    workspace: String,
    cwd: PathBuf,
    git_identity: String,
    path: std::ffi::OsString,
    launcher: PathBuf,
    configured_override: bool,
}

struct CatalogRequest {
    request_id: u64,
    input: QueryInput,
    sink: Arc<DashboardSink>,
}

pub(super) struct QueryRequest {
    pub(super) request_id: u64,
    pub(super) preset: String,
    pub(super) origin: AgentLaunchOrigin,
    pub(super) argv: Vec<std::ffi::OsString>,
    pub(super) project: String,
    pub(super) workspace: String,
}

#[derive(Default)]
struct CatalogQueueState {
    pending: Option<CatalogRequest>,
    attached: bool,
    stopped: bool,
}

#[derive(Default)]
pub(crate) struct CatalogQueue {
    state: Mutex<CatalogQueueState>,
    ready: Condvar,
    process_owner: Mutex<crate::catalog_process::CatalogProcessOwner>,
}

pub(super) struct CatalogShutdownPermit<'a> {
    owner: MutexGuard<'a, crate::catalog_process::CatalogProcessOwner>,
}

impl CatalogShutdownPermit<'_> {
    pub(super) fn close(&mut self) {
        self.owner.close();
    }
}

impl CatalogQueue {
    fn process_owner(
        &self,
    ) -> anyhow::Result<MutexGuard<'_, crate::catalog_process::CatalogProcessOwner>> {
        match self.process_owner.try_lock() {
            Ok(owner) => Ok(owner),
            Err(TryLockError::WouldBlock) => Err(crate::catalog_process::owner_error(
                "native model catalog process is active",
            )),
            Err(TryLockError::Poisoned(_)) => Err(crate::catalog_process::owner_error(
                "native model catalog process owner is unavailable",
            )),
        }
    }

    pub(super) fn with_process_owner<T>(
        &self,
        operation: impl FnOnce(&mut crate::catalog_process::CatalogProcessOwner) -> anyhow::Result<T>,
    ) -> anyhow::Result<T> {
        let mut owner = self.process_owner()?;
        if owner.is_closed() {
            return Err(crate::catalog_process::owner_error(
                "native model catalog process owner is closed",
            ));
        }
        owner.resolve_existing()?;
        operation(&mut owner)
    }

    pub(super) fn try_shutdown(&self) -> anyhow::Result<CatalogShutdownPermit<'_>> {
        let mut owner = self.process_owner()?;
        if owner.is_closed() {
            return Err(crate::catalog_process::owner_error(
                "native model catalog process owner is already closed",
            ));
        }
        owner.resolve_existing()?;
        Ok(CatalogShutdownPermit { owner })
    }

    pub(super) fn attach(&self) {
        let mut state = self.state.lock().unwrap();
        state.attached = true;
        self.ready.notify_all();
    }

    fn enqueue(&self, request: CatalogRequest) -> Result<Option<CatalogRequest>, ()> {
        let mut state = self.state.lock().unwrap();
        if !state.attached || state.stopped {
            return Err(());
        }
        let replaced = state.pending.replace(request);
        self.ready.notify_one();
        Ok(replaced)
    }

    fn next(&self, shutdown: &AtomicBool) -> Option<CatalogRequest> {
        let mut state = self.state.lock().unwrap();
        loop {
            if state.stopped || shutdown.load(std::sync::atomic::Ordering::Acquire) {
                state.pending.take();
                return None;
            }
            if let Some(request) = state.pending.take() {
                return Some(request);
            }
            state = self.ready.wait(state).unwrap();
        }
    }

    pub(super) fn stop(&self) {
        let mut state = self.state.lock().unwrap();
        state.stopped = true;
        state.pending.take();
        self.ready.notify_all();
    }
}

fn send_catalog_result(request: CatalogRequest, capabilities: AgentModelCapabilities) {
    let _ = request.sink.enqueue(DashboardOutbound {
        message: ServerMessage::Event(ServerEvent::AgentModels {
            request_id: request.request_id,
            capabilities,
        }),
        completion: None,
    });
}

pub(super) fn run(state: Arc<ServerState>) {
    let catalog_state = Arc::clone(&state);
    run_with(state, move |input, cancelled| {
        discover_capabilities(input, cancelled, &catalog_state.agent_model_catalog)
    });
}

fn run_with<F>(state: Arc<ServerState>, discover: F)
where
    F: Fn(&QueryInput, &AtomicBool) -> AgentModelCapabilities,
{
    while let Some(request) = state.agent_model_catalog.next(&state.shutdown) {
        let capabilities = discover(&request.input, &state.shutdown);
        if !state.shutdown.load(std::sync::atomic::Ordering::Acquire) {
            send_catalog_result(request, capabilities);
        }
    }
}

pub(super) fn request(
    state: &ServerState,
    dashboard: bool,
    owner: Option<&Arc<()>>,
    query: QueryRequest,
) -> Response {
    let QueryRequest {
        request_id,
        preset,
        origin,
        argv,
        project,
        workspace,
    } = query;
    let Some(owner) = owner else {
        return error_response(
            ErrorCode::InvalidRequest,
            "model lookup requires the current Dashboard",
        );
    };
    if !dashboard || !state.dashboard.owns(owner) {
        return error_response(
            ErrorCode::InvalidRequest,
            "model lookup requires the current Dashboard",
        );
    }
    let Some(snapshot) = state.dashboard.snapshot_owned(owner) else {
        return error_response(ErrorCode::Conflict, "Dashboard ownership changed");
    };
    let configured_override = state
        .settings
        .lock()
        .unwrap()
        .report
        .settings
        .agents
        .iter()
        .any(|agent| agent.name == preset);
    let workspace_record = state
        .registry
        .lock()
        .unwrap()
        .workspace(&project, &workspace)
        .cloned();
    let (Ok(workspace_record), Ok(launcher)) = (workspace_record, std::env::current_exe()) else {
        let _ = snapshot.sink.enqueue(DashboardOutbound {
            message: ServerMessage::Event(ServerEvent::AgentModels {
                request_id,
                capabilities: unsupported(
                    preset,
                    "The selected registered workspace is unavailable.",
                ),
            }),
            completion: None,
        });
        return Response::Ok;
    };
    let Some(git_identity) = workspace_record.git_identity else {
        let _ = snapshot.sink.enqueue(DashboardOutbound {
            message: ServerMessage::Event(ServerEvent::AgentModels {
                request_id,
                capabilities: unsupported(
                    preset,
                    "The registered workspace has no Git identity; explicit models are unavailable.",
                ),
            }),
            completion: None,
        });
        return Response::Ok;
    };
    let input = QueryInput {
        preset,
        origin,
        argv,
        project,
        workspace,
        cwd: workspace_record.path,
        git_identity,
        path: std::env::var_os("PATH").unwrap_or_default(),
        launcher,
        configured_override,
    };
    let queued = state.agent_model_catalog.enqueue(CatalogRequest {
        request_id,
        input,
        sink: snapshot.sink,
    });
    let Ok(replaced) = queued else {
        return error_response(
            ErrorCode::Internal,
            "could not start the model lookup worker",
        );
    };
    if let Some(request) = replaced {
        let capabilities = unsupported(
            request.input.preset.clone(),
            "A newer model lookup replaced this request.",
        );
        send_catalog_result(request, capabilities);
    }
    Response::Ok
}

fn discover_capabilities(
    input: &QueryInput,
    cancelled: &AtomicBool,
    catalog_queue: &CatalogQueue,
) -> AgentModelCapabilities {
    if input.origin != AgentLaunchOrigin::Detected {
        return unsupported(
            input.preset.clone(),
            "Only detected launchers can expose verified model choices.",
        );
    }
    if !matches!(
        input.preset.as_str(),
        "claude" | "pi" | "codex" | "omp" | "agy"
    ) {
        return unsupported(
            input.preset.clone(),
            "Verified explicit model selection is not available for this harness yet.",
        );
    }
    if input.configured_override {
        return unsupported(
            input.preset.clone(),
            "This named launcher is configured; its arguments and model policy are read-only.",
        );
    }
    let result = (|| -> anyhow::Result<_> {
        let executable = find_executable(&input.path, &input.preset)?;
        let native_argv = native_argv(&input.preset, &executable)?;
        let launch_argv = detected_launch_argv(&input.preset, &input.launcher, &native_argv);
        if !launch_matches(&input.argv, &launch_argv, false) {
            anyhow::bail!(
                "selected launcher does not match the Server-resolved executable and arguments"
            );
        }
        let actual_identity = crate::git::worktree_identity(&input.cwd)
            .context("registered Git worktree identity is unavailable")?;
        anyhow::ensure!(
            actual_identity == input.git_identity,
            "registered Git worktree changed; refresh the workspace"
        );
        let binding = binding(
            &input.preset,
            &launch_argv,
            &executable,
            &input.cwd,
            &input.project,
            &input.workspace,
            &input.git_identity,
        )?;
        let catalog = dashboard_catalog(
            catalog_queue,
            &input.preset,
            &executable,
            &native_argv,
            &input.cwd,
            cancelled,
        )?;
        let models = catalog
            .into_iter()
            .filter(|(alias, _)| input.preset == "codex" || alias != "default")
            .map(|(alias, resolved)| AgentModelOption { alias, resolved })
            .collect::<Vec<_>>();
        Ok((binding, models))
    })();
    match result {
        Ok((binding, models)) if !models.is_empty() => AgentModelCapabilities {
            preset: input.preset.clone(),
            binding: Some(binding),
            models,
            reason: None,
        },
        Ok(_) => unsupported(
            input.preset.clone(),
            format!(
                "{} returned no selectable models.",
                harness_label(&input.preset)
            ),
        ),
        Err(error) => unsupported(
            input.preset.clone(),
            format!(
                "{} model discovery is unavailable: {}",
                harness_label(&input.preset),
                error
            ),
        ),
    }
}

fn unsupported(preset: String, reason: impl Into<String>) -> AgentModelCapabilities {
    AgentModelCapabilities {
        preset,
        binding: None,
        models: Vec::new(),
        reason: Some(reason.into()),
    }
}

fn find_executable(path: &std::ffi::OsStr, name: &str) -> anyhow::Result<PathBuf> {
    for directory in std::env::split_paths(path) {
        let candidate = directory.join(name);
        if let Ok(metadata) = std::fs::metadata(&candidate)
            && metadata.is_file()
            && metadata.mode() & 0o111 != 0
        {
            return Ok(candidate);
        }
    }
    anyhow::bail!("native {name} executable is unavailable")
}

fn harness_label(preset: &str) -> &'static str {
    match preset {
        "claude" => "Claude Code",
        "pi" => "Pi",
        "codex" => "Codex",
        "omp" => "Oh My Pi",
        "agy" => "Antigravity CLI",
        _ => "Agent",
    }
}

fn native_argv(preset: &str, executable: &Path) -> anyhow::Result<Vec<std::ffi::OsString>> {
    anyhow::ensure!(
        matches!(preset, "claude" | "pi" | "codex" | "omp" | "agy"),
        "detected model selection is not supported for this harness"
    );
    let mut argv = vec![executable.as_os_str().to_owned()];
    ovrcr_protocol::apply_default_auto_trust(preset, &mut argv);
    Ok(argv)
}

fn detected_launch_argv(
    preset: &str,
    launcher: &Path,
    native_argv: &[std::ffi::OsString],
) -> Vec<std::ffi::OsString> {
    let mut argv = vec![
        launcher.as_os_str().to_owned(),
        "agent".into(),
        "run".into(),
        preset.into(),
        "--".into(),
    ];
    argv.extend(native_argv.iter().cloned());
    argv
}

fn dashboard_catalog(
    catalog_queue: &CatalogQueue,
    preset: &str,
    executable: &Path,
    native_argv: &[std::ffi::OsString],
    cwd: &Path,
    cancelled: &AtomicBool,
) -> anyhow::Result<Vec<(String, String)>> {
    match preset {
        "claude" => catalog_queue.with_process_owner(|owner| {
            crate::claude_delegation::dashboard_catalog(
                executable,
                native_argv,
                cwd,
                cancelled,
                owner,
            )
        }),
        "codex" => catalog_queue.with_process_owner(|owner| {
            crate::codex_delegation::dashboard_catalog(
                executable,
                native_argv,
                cwd,
                cancelled,
                owner,
            )
        }),
        "pi" => catalog_queue.with_process_owner(|owner| {
            crate::pi_model_catalog::dashboard_catalog(
                executable,
                native_argv,
                cwd,
                cancelled,
                owner,
            )
        }),
        "omp" => catalog_queue.with_process_owner(|owner| {
            crate::omp_model_catalog::dashboard_catalog(
                executable,
                native_argv,
                cwd,
                cancelled,
                owner,
            )
        }),
        "agy" => catalog_queue.with_process_owner(|owner| {
            agy_dashboard_catalog(executable, native_argv, cwd, cancelled, owner)
        }),
        _ => anyhow::bail!("verified model discovery is not available for this harness"),
    }
}

const AGY_CATALOG_LIMIT: usize = 512 * 1024;
const AGY_DIAGNOSTIC_LIMIT: usize = 64 * 1024;

fn agy_dashboard_catalog(
    executable: &Path,
    native_argv: &[std::ffi::OsString],
    cwd: &Path,
    cancelled: &AtomicBool,
    owner: &mut crate::catalog_process::CatalogProcessOwner,
) -> anyhow::Result<Vec<(String, String)>> {
    let mut expected = vec![executable.as_os_str().to_owned()];
    ovrcr_protocol::apply_default_auto_trust("agy", &mut expected);
    anyhow::ensure!(
        native_argv == expected,
        "Antigravity CLI launch options are not the detected managed configuration"
    );
    let executable = executable
        .canonicalize()
        .context("native Antigravity CLI executable unavailable")?;
    anyhow::ensure!(
        executable.is_file(),
        "native Antigravity CLI executable unavailable"
    );
    let cwd = cwd
        .canonicalize()
        .context("Antigravity CLI workspace is unavailable")?;
    anyhow::ensure!(cwd.is_dir(), "Antigravity CLI workspace is unavailable");

    // Native model discovery is a separate `agy models` command. Do not pass
    // interactive permission flags or OVRCR's session/reporting authority.
    let mut command = std::process::Command::new(&executable);
    command.current_dir(cwd).arg("models");
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
        "OVRCR_CLAUDE_ARTIFACTS",
    ] {
        command.env_remove(name);
    }

    let output = owner.run_command(
        "Antigravity CLI",
        &mut command,
        AGY_CATALOG_LIMIT,
        AGY_DIAGNOSTIC_LIMIT,
        std::time::Duration::from_secs(20),
        cancelled,
    )?;
    let text = String::from_utf8(output.stdout)
        .context("native Antigravity CLI model catalog is not UTF-8")?;
    parse_agy_model_list(&text)
}

fn parse_agy_model_list(output: &str) -> anyhow::Result<Vec<(String, String)>> {
    anyhow::ensure!(
        output.len() <= AGY_CATALOG_LIMIT,
        "native Antigravity CLI model discovery output bound"
    );
    let mut models = std::collections::BTreeMap::new();
    for line in output.lines() {
        if line.is_empty() {
            continue;
        }
        anyhow::ensure!(
            line.len() <= 4096,
            "native Antigravity CLI model catalog line bound"
        );
        let Some((id, label)) = line.split_once('\t') else {
            anyhow::bail!("native Antigravity CLI model catalog is not tab-separated");
        };
        anyhow::ensure!(
            !label.contains('\t') && valid_agy_model_id(id),
            "native Antigravity CLI model catalog contains an invalid model ID"
        );
        anyhow::ensure!(
            !label.is_empty()
                && label.len() <= 1024
                && label.trim() == label
                && !label.chars().any(char::is_control),
            "native Antigravity CLI model catalog contains an invalid display label"
        );
        anyhow::ensure!(
            models.len() < 8192,
            "native Antigravity CLI model catalog entry bound"
        );
        anyhow::ensure!(
            models.insert(id.to_owned(), label.to_owned()).is_none(),
            "native Antigravity CLI model catalog contains a duplicate model ID"
        );
    }
    Ok(models.into_iter().collect())
}

fn valid_agy_model_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 256
        && id.as_bytes()[0].is_ascii_alphanumeric()
        && id.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(byte, b'.' | b'_' | b'-' | b':' | b'/' | b'@' | b'+')
        })
}

/// Detected managed argv has a fixed wrapper layout. Compare every argument
/// byte-for-byte except the wrapper and native executable slots, whose paths
/// may use different symlink spellings for the same files.
pub(super) fn launch_matches(
    selected: &[std::ffi::OsString],
    expected: &[std::ffi::OsString],
    allow_task: bool,
) -> bool {
    if expected.len() < 6 {
        return false;
    }
    let has_standard_task = allow_task
        && selected.len() == expected.len() + 2
        && selected.get(expected.len()).is_some_and(|arg| arg == "--");
    let has_agy_task = allow_task
        && expected.get(3).is_some_and(|arg| arg == "agy")
        && selected.len() == expected.len() + 1
        && selected
            .get(expected.len())
            .and_then(|arg| arg.to_str())
            .is_some_and(|arg| {
                arg.strip_prefix("--prompt-interactive=")
                    .is_some_and(|prompt| !prompt.is_empty())
            });
    if selected.len() != expected.len() && !has_standard_task && !has_agy_task {
        return false;
    }
    let Some(selected_launcher) = selected
        .first()
        .and_then(|arg| Path::new(arg).canonicalize().ok())
    else {
        return false;
    };
    let Some(expected_launcher) = expected
        .first()
        .and_then(|arg| Path::new(arg).canonicalize().ok())
    else {
        return false;
    };
    let Some(selected_native) = selected
        .get(5)
        .and_then(|arg| Path::new(arg).canonicalize().ok())
    else {
        return false;
    };
    let Some(expected_native) = expected
        .get(5)
        .and_then(|arg| Path::new(arg).canonicalize().ok())
    else {
        return false;
    };
    selected_launcher == expected_launcher
        && selected_native == expected_native
        && selected
            .iter()
            .zip(expected)
            .enumerate()
            .all(|(index, (selected, expected))| index == 0 || index == 5 || selected == expected)
}

pub(super) fn prepare_initial_prompt_session(
    state: &ServerState,
    request: CreateAgentSessionRequest,
) -> anyhow::Result<CreateSessionRequest> {
    let preset = request
        .session
        .label
        .as_deref()
        .filter(|preset| matches!(*preset, "claude" | "pi" | "codex" | "omp" | "agy"))
        .context("Initial prompts require a supported detected harness")?;
    anyhow::ensure!(
        !state
            .settings
            .lock()
            .unwrap()
            .report
            .settings
            .agents
            .iter()
            .any(|agent| agent.name == preset),
        "configured launchers do not support Initial prompts"
    );
    anyhow::ensure!(
        matches!(request.session.kind, SessionKind::Agent { ref name } if name == preset),
        "Initial prompts require a matching detected agent harness"
    );
    let executable = find_executable(&std::env::var_os("PATH").unwrap_or_default(), preset)?;
    let launcher = std::env::current_exe()?;
    let native = native_argv(preset, &executable)?;
    let expected = detected_launch_argv(preset, &launcher, &native);
    anyhow::ensure!(
        request.session.label.as_deref() == Some(preset)
            && launch_matches(&request.session.argv, &expected, false),
        "Initial prompts require the current detected launcher and arguments"
    );
    prepare_session(state, request)
}

fn binding(
    preset: &str,
    launch_argv: &[std::ffi::OsString],
    executable: &Path,
    cwd: &Path,
    project: &str,
    workspace: &str,
    git_identity: &str,
) -> anyhow::Result<String> {
    let executable = executable.canonicalize()?;
    let cwd = cwd.canonicalize()?;
    let executable_meta = std::fs::metadata(&executable)?;
    let argv = launch_argv
        .iter()
        .enumerate()
        .map(|(index, arg)| {
            if index == 0 || index == 5 {
                Path::new(arg)
                    .canonicalize()
                    .map(|path| hex(path.as_os_str().as_bytes()))
            } else {
                Ok(hex(arg.as_bytes()))
            }
        })
        .collect::<std::io::Result<Vec<_>>>()?;
    let identity = serde_json::json!({
        "preset":preset,
        "origin":"detected",
        "argv":argv,
        "executable":executable,
        "executable_file":[executable_meta.dev(), executable_meta.ino(), executable_meta.len(), executable_meta.mtime(), executable_meta.mtime_nsec()],
        "cwd":cwd,
        "workspace_identity":[project, workspace],
        "git_identity":git_identity,
    });
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&identity)?)
    ))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Revalidate and compose an explicit policy before the session mutation lock
/// is acquired. Harness default returns the original session unchanged.
pub(super) fn prepare_session(
    state: &ServerState,
    mut request: CreateAgentSessionRequest,
) -> anyhow::Result<CreateSessionRequest> {
    let AgentModelPolicy::Explicit(model) = &request.model_policy else {
        anyhow::ensure!(
            request.model_binding.is_none() && request.expected_model.is_none(),
            "Harness default cannot carry an explicit model binding"
        );
        return Ok(request.session);
    };
    anyhow::ensure!(request.model_policy.is_valid(), "explicit model is invalid");
    let preset = request
        .session
        .label
        .as_deref()
        .filter(|preset| matches!(*preset, "claude" | "pi" | "codex" | "omp" | "agy"))
        .context("explicit model selection is not supported for this harness")?;
    anyhow::ensure!(
        matches!(request.session.kind, SessionKind::Agent { ref name } if name == preset),
        "explicit model selection requires the matching detected harness label"
    );
    anyhow::ensure!(
        !state
            .settings
            .lock()
            .unwrap()
            .report
            .settings
            .agents
            .iter()
            .any(|agent| agent.name == preset),
        "configured launchers do not support explicit model overrides"
    );
    let executable = find_executable(&std::env::var_os("PATH").unwrap_or_default(), preset)?;
    let native_argv = native_argv(preset, &executable)?;
    let launcher = std::env::current_exe()?;
    let expected = detected_launch_argv(preset, &launcher, &native_argv);
    anyhow::ensure!(
        launch_matches(&request.session.argv, &expected, true),
        "selected launch configuration changed or is unsupported"
    );
    let workspace = state
        .registry
        .lock()
        .unwrap()
        .workspace(&request.session.project, &request.session.workspace)?
        .clone();
    let git_identity = workspace
        .git_identity
        .as_deref()
        .context("registered Git worktree identity is missing")?;
    anyhow::ensure!(
        crate::git::worktree_identity(&workspace.path)? == git_identity,
        "registered Git worktree changed; refresh the workspace"
    );
    let current_binding = binding(
        preset,
        &expected,
        &executable,
        &workspace.path,
        &request.session.project,
        &request.session.workspace,
        git_identity,
    )?;
    anyhow::ensure!(
        request.model_binding.as_deref() == Some(current_binding.as_str()),
        "model capability is stale; refresh the model list"
    );
    if matches!(preset, "pi" | "omp") {
        anyhow::ensure!(
            ovrcr_protocol::DelegationSettings::valid_model(model),
            "{preset} explicit models require an exact provider/model ID"
        );
    }
    let catalog = dashboard_catalog(
        &state.agent_model_catalog,
        preset,
        &executable,
        &native_argv,
        &workspace.path,
        &state.shutdown,
    )?;
    let resolved = catalog
        .iter()
        .find(|(alias, _)| alias == model && alias != "default")
        .map(|(_, resolved)| resolved);
    anyhow::ensure!(
        resolved.is_some() && request.expected_model.as_ref() == resolved,
        "selected {preset} model resolution changed; choose the current model"
    );
    request
        .session
        .argv
        .splice(6..6, ["--model".into(), model.clone().into()]);
    Ok(request.session)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    use std::process::{Command, Stdio};
    use std::thread;
    use std::time::{Duration, Instant};

    fn discover_for_test(input: &QueryInput) -> AgentModelCapabilities {
        let catalog_queue = CatalogQueue::default();
        discover_capabilities(input, &AtomicBool::new(false), &catalog_queue)
    }

    fn executable(path: &Path) {
        std::fs::write(path, "#!/bin/sh\nexit 0\n").unwrap();
        let mut permissions = std::fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).unwrap();
    }

    #[test]
    fn agy_catalog_parser_requires_bounded_unique_tsv_ids_and_labels() {
        assert_eq!(
            parse_agy_model_list(
                "gemini-4-argon-high\tGemini 4 Argon (High)\n\
                 gemini-3.8-flash-low\tGemini 3.8 Flash (Low)\n"
            )
            .unwrap(),
            vec![
                (
                    "gemini-3.8-flash-low".into(),
                    "Gemini 3.8 Flash (Low)".into()
                ),
                ("gemini-4-argon-high".into(), "Gemini 4 Argon (High)".into()),
            ]
        );

        for malformed in [
            "Fetching available models...\n",
            "model\t\n",
            "-leading-hyphen\tInvalid ID\n",
            "gemini-4\tLabel\textra\n",
            "gemini-4\t Label with padding \n",
            "gemini-4\tLabel\ngemini-4\tDuplicate\n",
        ] {
            assert!(
                parse_agy_model_list(malformed).is_err(),
                "malformed native output must fail closed: {malformed:?}"
            );
        }
    }

    #[test]
    fn agy_managed_launch_match_accepts_one_prompt_interactive_argument() {
        let root = tempfile::tempdir().unwrap();
        let wrapper = root.path().join("ovrcr");
        let native = root.path().join("agy");
        executable(&wrapper);
        executable(&native);
        let mut native_argv = vec![native.into_os_string()];
        ovrcr_protocol::apply_default_auto_trust("agy", &mut native_argv);
        let mut expected = vec![
            wrapper.into_os_string(),
            "agent".into(),
            "run".into(),
            "agy".into(),
            "--".into(),
        ];
        expected.extend(native_argv);
        let prompt = "-fix first\nthen treat --mode=plan as text";
        let mut selected = expected.clone();
        selected.push(format!("--prompt-interactive={prompt}").into());

        assert!(launch_matches(&selected, &expected, true));
        assert!(!launch_matches(&selected, &expected, false));
        selected.pop();
        selected.push("--prompt-interactive=".into());
        assert!(!launch_matches(&selected, &expected, true));
        selected.pop();
        selected.push("--prompt-interactive=one".into());
        selected.push("--prompt-interactive=two".into());
        assert!(!launch_matches(&selected, &expected, true));
    }

    #[test]
    fn unresolved_catalog_child_blocks_repeated_spawns_and_bounded_shutdown_until_waited() {
        let root = tempfile::tempdir().unwrap();
        let ready = root.path().join("ready");
        let release = root.path().join("release");
        let refused_spawn = root.path().join("must-not-spawn");
        let recovered_spawn = root.path().join("recovered-spawn");
        let mut child = Command::new("/bin/sh")
            .args([
                "-c",
                &format!(
                    "printf ready > '{}'; while [ ! -e '{}' ]; do /bin/sleep 0.01; done",
                    ready.display(),
                    release.display()
                ),
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let pid = child.id() as libc::pid_t;
        let capture =
            crate::catalog_process::CatalogCapture::take_from_child(&mut child, 1024, 1024);
        let ready_deadline = Instant::now() + Duration::from_secs(2);
        while !ready.exists() && Instant::now() < ready_deadline {
            thread::sleep(Duration::from_millis(5));
        }
        if !ready.exists() || unsafe { libc::getpgid(pid) } == pid {
            let _ = std::fs::write(&release, "release\n");
            let _ = child.wait();
            panic!("synthetic unresolved catalog child did not start in a nonmatching group");
        }

        let queue = CatalogQueue::default();
        let adopted = queue.process_owner.lock().unwrap().retain_unresolved_child(
            "synthetic",
            child,
            capture,
        );
        let shutdown_started = Instant::now();
        let shutdown_refused = queue.try_shutdown().is_err();
        let shutdown_elapsed = shutdown_started.elapsed();
        let child_still_owned = unsafe { libc::kill(pid, 0) } == 0;

        let mut command = Command::new("/bin/sh");
        command
            .arg("-c")
            .arg(format!("printf spawned > '{}'", refused_spawn.display()));
        let repeated_request_refused = queue
            .with_process_owner(|owner| {
                owner.run_command(
                    "synthetic",
                    &mut command,
                    1024,
                    1024,
                    Duration::from_secs(1),
                    &AtomicBool::new(false),
                )
            })
            .is_err();

        std::fs::write(&release, "release\n").unwrap();
        let mut command = Command::new("/bin/sh");
        command.arg("-c").arg(format!(
            "printf recovered > '{}'",
            recovered_spawn.display()
        ));
        let recovered = queue.with_process_owner(|owner| {
            let waited_pid_absent = unsafe { libc::kill(pid, 0) } == -1
                && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH);
            let waited_group_absent = unsafe { libc::kill(-pid, 0) } == -1
                && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH);
            let output = owner.run_command(
                "synthetic",
                &mut command,
                1024,
                1024,
                Duration::from_secs(1),
                &AtomicBool::new(false),
            )?;
            Ok((output, waited_pid_absent, waited_group_absent))
        });
        let final_shutdown = queue.try_shutdown();
        let closed = final_shutdown.is_ok();
        if let Ok(mut permit) = final_shutdown {
            permit.close();
        }
        let after_close_refused = queue.with_process_owner(|_| Ok(())).is_err();

        assert!(adopted.is_ok());
        assert!(shutdown_refused, "uncertain cleanup must refuse shutdown");
        assert!(shutdown_elapsed < Duration::from_secs(3));
        assert!(child_still_owned, "failed shutdown must retain its Child");
        assert!(
            repeated_request_refused,
            "uncertain owner must gate later spawns"
        );
        assert!(
            !refused_spawn.exists(),
            "a repeated request spawned a second child"
        );
        let (_output, waited_pid_absent, waited_group_absent) = recovered
            .expect("a later exit observation must recover the owner and permit a new spawn");
        assert!(recovered_spawn.exists());
        assert!(
            waited_pid_absent,
            "recovery must wait the exact retained Child"
        );
        assert!(
            waited_group_absent,
            "recovery must prove the exact retained process group absent before a new spawn"
        );
        assert!(closed, "shutdown should proceed after exact cleanup");
        assert!(
            after_close_refused,
            "accepted shutdown must close catalog admission"
        );
    }

    #[test]
    fn executable_resolution_skips_path_entries_without_the_launcher() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let executable = second.path().join("claude");
        std::fs::write(&executable, "#!/bin/sh\nexit 0\n").unwrap();
        let mut permissions = std::fs::metadata(&executable).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&executable, permissions).unwrap();
        let path = std::env::join_paths([first.path(), second.path()]).unwrap();

        assert_eq!(find_executable(&path, "claude").unwrap(), executable);
    }

    #[test]
    fn managed_launch_match_allows_executable_symlink_spelling_only() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let wrapper = root.path().join("ovrcr");
        let native = root.path().join("claude");
        executable(&wrapper);
        executable(&native);
        let wrapper_alias = root.path().join("ovrcr-alias");
        let native_alias = root.path().join("claude-alias");
        symlink(&wrapper, &wrapper_alias).unwrap();
        symlink(&native, &native_alias).unwrap();

        let expected = vec![
            wrapper.into_os_string(),
            "agent".into(),
            "run".into(),
            "claude".into(),
            "--".into(),
            native.into_os_string(),
            "--dangerously-skip-permissions".into(),
        ];
        let mut selected = expected.clone();
        selected[0] = wrapper_alias.into_os_string();
        selected[5] = native_alias.into_os_string();

        assert!(launch_matches(&selected, &expected, false));
        selected[6] = "--different-policy".into();
        assert!(!launch_matches(&selected, &expected, false));
    }

    #[test]
    fn replaced_or_unmarked_worktree_is_refused_before_catalog_spawn() {
        for replacement in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let repo = root.path().join("repo");
            std::fs::create_dir(&repo).unwrap();
            let initialized = std::process::Command::new("git")
                .args(["init", "--quiet"])
                .current_dir(&repo)
                .status()
                .unwrap();
            assert!(initialized.success());
            let registered_identity = crate::git::capture_worktree_identity(&repo).unwrap();
            let marker = repo.join(".git/ovrcr-identity");
            if replacement {
                std::fs::write(&marker, "replacement-generation\n").unwrap();
            } else {
                std::fs::remove_file(&marker).unwrap();
            }

            let bin = root.path().join("bin");
            std::fs::create_dir(&bin).unwrap();
            let called = root.path().join("native-called");
            let native = bin.join("claude");
            std::fs::write(
                &native,
                format!("#!/bin/sh\nprintf called > '{}'\n", called.display()),
            )
            .unwrap();
            let mut permissions = std::fs::metadata(&native).unwrap().permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&native, permissions).unwrap();
            let launcher = std::env::current_exe().unwrap();
            let mut native_argv = vec![native.as_os_str().to_owned()];
            ovrcr_protocol::apply_default_auto_trust("claude", &mut native_argv);
            let mut argv = vec![
                launcher.as_os_str().to_owned(),
                "agent".into(),
                "run".into(),
                "claude".into(),
                "--".into(),
            ];
            argv.extend(native_argv);
            let input = QueryInput {
                preset: "claude".into(),
                origin: AgentLaunchOrigin::Detected,
                argv,
                project: "p".into(),
                workspace: "w".into(),
                cwd: repo,
                git_identity: registered_identity,
                path: std::env::join_paths([&bin]).unwrap(),
                launcher,
                configured_override: false,
            };

            let capabilities = discover_for_test(&input);
            assert!(capabilities.models.is_empty());
            assert!(
                capabilities
                    .reason
                    .as_deref()
                    .is_some_and(|reason| reason.contains("registered Git worktree"))
            );
            assert!(!called.exists(), "invalid worktree must not run Claude");
        }
    }

    #[test]
    fn model_catalog_refuses_a_client_launcher_that_differs_from_server_resolution() {
        let root = tempfile::tempdir().unwrap();
        let bin = root.path().join("bin");
        std::fs::create_dir(&bin).unwrap();
        executable(&bin.join("claude"));
        let cwd = root.path().join("workspace");
        std::fs::create_dir(&cwd).unwrap();
        let path = std::env::join_paths([&bin]).unwrap();
        let input = QueryInput {
            preset: "claude".into(),
            origin: AgentLaunchOrigin::Detected,
            argv: vec![
                "/fixture/ovrcr".into(),
                "agent".into(),
                "run".into(),
                "claude".into(),
                "--".into(),
                "/client/path/claude".into(),
                "--dangerously-skip-permissions".into(),
            ],
            project: "p".into(),
            workspace: "w".into(),
            cwd,
            git_identity: "test-identity".into(),
            path,
            launcher: PathBuf::from("/fixture/ovrcr"),
            configured_override: false,
        };

        let capabilities = discover_for_test(&input);
        assert!(capabilities.binding.is_none());
        assert!(capabilities.models.is_empty());
        assert!(
            capabilities
                .reason
                .as_deref()
                .is_some_and(|reason| reason.contains("does not match"))
        );
    }

    #[test]
    fn configured_origin_never_reaches_native_model_discovery() {
        for preset in ["claude", "omp"] {
            let capabilities = discover_for_test(&QueryInput {
                preset: preset.into(),
                origin: AgentLaunchOrigin::Configured,
                argv: Vec::new(),
                project: "p".into(),
                workspace: "w".into(),
                cwd: PathBuf::new(),
                git_identity: String::new(),
                path: std::ffi::OsString::new(),
                launcher: PathBuf::new(),
                configured_override: false,
            });
            assert!(capabilities.binding.is_none());
            assert!(capabilities.models.is_empty());
            assert!(
                capabilities
                    .reason
                    .as_deref()
                    .is_some_and(|reason| reason.contains("Only detected launchers"))
            );
        }
    }

    #[test]
    fn harness_default_preserves_the_complete_selected_launcher_argv() {
        let state = super::super::tests::test_state(None, None);
        let argv = vec![
            "/fixture/ovrcr".into(),
            "agent".into(),
            "run".into(),
            "custom".into(),
            "--".into(),
            "/fixture/native".into(),
            "--configured-flag".into(),
        ];
        let prepared = prepare_session(
            &state,
            CreateAgentSessionRequest {
                session: CreateSessionRequest {
                    project: "missing".into(),
                    workspace: "missing".into(),
                    name: "default-policy".into(),
                    argv: argv.clone(),
                    label: Some("custom".into()),
                    kind: SessionKind::Agent {
                        name: "custom".into(),
                    },
                },
                model_policy: AgentModelPolicy::HarnessDefault,
                model_binding: None,
                expected_model: None,
            },
        )
        .unwrap();
        assert_eq!(prepared.argv, argv);
    }

    #[test]
    fn server_shutdown_joins_the_owned_catalog_worker_and_process_group() {
        use std::sync::atomic::Ordering;
        use std::time::Instant;

        let root = tempfile::tempdir().unwrap();
        let pid_file = root.path().join("pid");
        let script = root.path().join("held-catalog");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nprintf '%s' \"$$\" > {}\nexec /bin/sleep 30\n",
                pid_file.display()
            ),
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script, permissions).unwrap();

        let state = super::super::tests::test_state(None, None);
        let child_script = script.clone();
        let child_cwd = root.path().to_path_buf();
        let worker_state = Arc::clone(&state);
        state.agent_model_catalog.attach();
        let worker = thread::spawn(move || {
            run_with(worker_state, move |_, shutdown| {
                let mut command = std::process::Command::new(&child_script);
                command.current_dir(&child_cwd);
                let result =
                    crate::claude_delegation::initialized_catalog_cancellable(command, shutdown);
                assert_eq!(
                    result.unwrap_err().to_string(),
                    "native model discovery cancelled"
                );
                unsupported("claude".into(), "test catalog cancelled")
            });
        });
        let request = CatalogRequest {
            request_id: 1,
            input: QueryInput {
                preset: "claude".into(),
                origin: AgentLaunchOrigin::Detected,
                argv: Vec::new(),
                project: "p".into(),
                workspace: "w".into(),
                cwd: root.path().to_path_buf(),
                git_identity: "test-identity".into(),
                path: std::ffi::OsString::new(),
                launcher: PathBuf::new(),
                configured_override: false,
            },
            sink: DashboardSink::new(),
        };
        let deadline = Instant::now() + Duration::from_secs(2);
        state
            .agent_model_catalog
            .enqueue(request)
            .expect("catalog queue is attached before the worker starts");
        let script_pid: i32 = loop {
            if let Some(pid) = std::fs::read_to_string(&pid_file)
                .ok()
                .and_then(|contents| contents.parse().ok())
            {
                break pid;
            }
            assert!(
                Instant::now() < deadline,
                "fake catalog did not publish its PID"
            );
            thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(unsafe { libc::getpgid(script_pid) }, script_pid);
        state.shutdown.store(true, Ordering::Release);
        state.agent_model_catalog.stop();
        assert!(worker.join().is_ok(), "catalog worker did not join");
    }
}

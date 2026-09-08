# OVRCR Operator Agent Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A pinned "OVRCR operator" entry at the top of the dashboard sidebar that runs interactive Pi in a PTY with an OVRCR extension, so the user can manage terminals and agents by talking to it.

**Architecture:** One reserved session kind owned by the server. `Request::StartOperator` spawns an ordinary PTY session with a fixed Pi argv and `kind: Operator`; every other mechanism (output, history, detach, pause, kill) is the existing session machinery. The extension inside Pi implements tools by running `ovrcr --json`, and the dashboard pins the operator summary above the project tree.

**Tech Stack:** Rust 2024 workspace (`ovrcr-protocol`, `ovrcr-runtime`, `ovrcr-tui`, root `ovrcr`), bincode wire protocol with a version preamble, Pi 0.84.4 interactive mode with a JavaScript extension (`pi.registerTool`, `ctx.ui.confirm`, `pi.registerCommand`), ratatui dashboard.

**Spec:** `plans/2026-09-08-operator-agent-design.md`

## Global Constraints

- Blocking I/O and threads only; no async runtime.
- Any change to a serialized type in `crates/ovrcr-protocol` bumps `PROTOCOL_VERSION` in `crates/ovrcr-protocol/src/codec.rs` and regenerates `EXPECTED` in the `wire_snapshot` test in `wire.rs` (run `cargo test -p ovrcr-protocol wire_snapshot -- --nocapture` and paste the printed block).
- The operator is never passed to `git worktree remove` or `prune`; it has no workspace.
- Pi baseline is 0.84.4. Argv flags used: `--no-extensions --extension --no-context-files --no-skills --no-prompt-templates --exclude-tools --system-prompt --session-dir --session-id --name --tui-mode --provider --model --thinking`, all present in `pi --help` for that version.
- Confirmations are required for: `close_terminal`, `kill_terminal`, `remove_workspace`, `remove_project`, `run_cancel`, `task_delete`, `server_shutdown`, `bash`. No confirmation for create, send, pause, resume, task run, task pause, task resume, or any read.
- `SendTerminal` to the session the dashboard has in terminal mode returns `Conflict` with the message `that terminal is receiving keyboard input from the dashboard`.
- Before every commit: `rtk proxy cargo fmt --all`, `rtk proxy cargo clippy --all-targets --all-features -- -D warnings`, and the task's named tests. Commit messages end with the repository's attribution trailer.
- Prefix executables with `rtk proxy` in shell commands per the repository convention.

## File Structure

| File | Responsibility |
| --- | --- |
| `crates/ovrcr-protocol/src/session.rs` | `SessionKind` on `SessionSummary` |
| `crates/ovrcr-protocol/src/wire.rs` | `HierarchySnapshot.operator`, `Request::StartOperator`, `DashboardSelection`, `DashboardMode`, snapshot test |
| `crates/ovrcr-protocol/src/registry.rs` | `OperatorConfig` table on `Registry` |
| `crates/ovrcr-runtime/src/server/operator.rs` (new) | Pi resolution, operator directory, argv, `start_operator` |
| `crates/ovrcr-runtime/src/server/mod.rs` | operator state, spawn cwd override, guards, selection tracking, send refusal |
| `crates/ovrcr-runtime/src/server/connections.rs` | request dispatch for the three new requests |
| `src/pi-operator-extension.mjs` (new) | Pi extension: tools, confirmations, `/status`, presence |
| `src/operator-system-prompt.md` (new) | operator instructions |
| `src/cli/args.rs`, `src/cli/mod.rs`, `src/cli/resources.rs`, `src/cli/output.rs` | `operator start|stop`, `terminal selected`, kind in listings |
| `crates/ovrcr-tui/src/dashboard/state.rs`, `render.rs`, `mod.rs` | `TreeRow::Operator`, `o`, Enter-to-start, mode reporting |
| `tests/operator_extension.mjs` (new), `tests/operator_extension.rs` (new) | extension contract test under Node |
| `tests/server_lifecycle.rs`, `tests/tui.rs`, `tests/terminal_acceptance.rs`, `tests/resource_cli.rs` | integration tests |
| `README.md`, `docs/testing-computer-use.md` | user and smoke-check documentation |

---

### Task 1: Protocol types

**Files:**
- Modify: `crates/ovrcr-protocol/src/session.rs`
- Modify: `crates/ovrcr-protocol/src/wire.rs`
- Modify: `crates/ovrcr-protocol/src/registry.rs`
- Modify: `crates/ovrcr-protocol/src/codec.rs` (version bump)
- Modify: every `SessionSummary { .. }` and `HierarchySnapshot { .. }` literal the compiler reports (`codec.rs`, `wire.rs`, `crates/ovrcr-runtime/src/session/mod.rs`, `crates/ovrcr-runtime/src/server/mod.rs`, `crates/ovrcr-runtime/src/server/tests.rs`, `crates/ovrcr-tui/src/dashboard/state.rs`, `tests/server_lifecycle.rs`, `tests/service.rs`, `tests/tui.rs`)
- Test: `crates/ovrcr-protocol/src/wire.rs`, `crates/ovrcr-protocol/src/registry.rs`

**Interfaces:**
- Produces: `SessionKind { User, Operator }` (Copy, Default = User); `SessionSummary.kind: SessionKind`; `HierarchySnapshot.operator: Option<SessionSummary>`; `Request::StartOperator`, `Request::DashboardSelection`, `Request::DashboardMode { terminal: bool }`; `OperatorConfig { provider, model, thinking, executable }` as `Registry.operator`.

- [ ] **Step 1: Write the failing registry test**

Append to the `tests` module in `crates/ovrcr-protocol/src/registry.rs`:

```rust
#[test]
fn operator_table_is_optional_and_round_trips() {
    let empty: Registry = toml::from_str("").unwrap();
    assert_eq!(empty.operator, OperatorConfig::default());
    let configured: Registry = toml::from_str(
        "[operator]\nprovider = \"anthropic\"\nmodel = \"claude-sonnet-4-5\"\nthinking = \"low\"\nexecutable = \"/opt/pi/bin/pi\"\n",
    )
    .unwrap();
    assert_eq!(configured.operator.provider.as_deref(), Some("anthropic"));
    assert_eq!(configured.operator.model.as_deref(), Some("claude-sonnet-4-5"));
    assert_eq!(configured.operator.thinking.as_deref(), Some("low"));
    assert_eq!(
        configured.operator.executable.as_deref(),
        Some(std::path::Path::new("/opt/pi/bin/pi"))
    );
    let text = toml::to_string(&configured).unwrap();
    assert!(text.contains("[operator]"));
}
```

`toml` is already a dependency of the runtime crate but not the protocol crate; add `toml = { workspace = true }` under `[dev-dependencies]` in `crates/ovrcr-protocol/Cargo.toml`.

- [ ] **Step 2: Run it to verify it fails**

Run: `rtk proxy cargo test -p ovrcr-protocol operator_table_is_optional_and_round_trips`
Expected: compile error, `OperatorConfig` not found.

- [ ] **Step 3: Add the types**

In `crates/ovrcr-protocol/src/session.rs`, after `AgentActivity`:

```rust
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionKind {
    #[default]
    User,
    Operator,
}
```

and add `pub kind: SessionKind,` as the last field of `SessionSummary`.

In `crates/ovrcr-protocol/src/registry.rs`:

```rust
/// Settings for the OVRCR operator agent. Every field falls back to Pi's own
/// default when unset.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OperatorConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executable: Option<PathBuf>,
}
```

and on `Registry`:

```rust
pub struct Registry {
    pub projects: Vec<ProjectRecord>,
    #[serde(default, skip_serializing_if = "operator_is_default")]
    pub operator: OperatorConfig,
}

fn operator_is_default(config: &OperatorConfig) -> bool {
    *config == OperatorConfig::default()
}
```

Export it from `crates/ovrcr-protocol/src/lib.rs`: add `OperatorConfig` to the `registry` re-export and `SessionKind` to the `session` re-export.

In `crates/ovrcr-protocol/src/wire.rs`: add `pub operator: Option<SessionSummary>,` to `HierarchySnapshot`, and append to the **end** of `Request` (never mid-enum):

```rust
    StartOperator,
    DashboardSelection,
    DashboardMode {
        terminal: bool,
    },
```

In `crates/ovrcr-protocol/src/codec.rs` set `pub const PROTOCOL_VERSION: u32 = 3;`.

- [ ] **Step 4: Fix every struct literal**

Run `rtk proxy cargo check --all-targets`. For each `SessionSummary { .. }` literal add `kind: SessionKind::User,`; for each `HierarchySnapshot { .. }` literal add `operator: None,`. In `crates/ovrcr-runtime/src/server/mod.rs` the `snapshot_from_state` function builds `HierarchySnapshot { projects }`; make it `HierarchySnapshot { projects, operator: None }` for now (Task 2 fills it). In `crates/ovrcr-runtime/src/session/mod.rs` `spawn_internal` builds the summary; give `SessionSpec` a new field `pub kind: SessionKind` and copy it through (`kind: spec.kind`); every `SessionSpec { .. }` literal gets `kind: SessionKind::User,`.

- [ ] **Step 5: Regenerate the wire snapshot**

Run: `rtk proxy cargo test -p ovrcr-protocol wire_snapshot -- --nocapture`. It fails and prints the new block. Add the three new requests to `requests()` in the `wire_snapshot` module before regenerating:

```rust
            ("StartOperator", Request::StartOperator),
            ("DashboardSelection", Request::DashboardSelection),
            ("DashboardMode", Request::DashboardMode { terminal: true }),
```

Replace `EXPECTED` with the printed block. Run again; it passes.

- [ ] **Step 6: Run the protocol and workspace tests**

Run: `rtk proxy cargo test -p ovrcr-protocol` then `rtk proxy cargo test --workspace`
Expected: all pass (behavior is unchanged; only shapes grew).

- [ ] **Step 7: Commit**

```bash
git add crates/ovrcr-protocol crates/ovrcr-runtime crates/ovrcr-tui tests
git commit -m "feat(protocol): add the operator session kind, requests, and config"
```

---

### Task 2: Server: operator state, spawn, guards, and context requests

**Files:**
- Create: `crates/ovrcr-runtime/src/server/operator.rs`
- Modify: `crates/ovrcr-runtime/src/server/mod.rs`
- Modify: `crates/ovrcr-runtime/src/server/connections.rs`
- Modify: `crates/ovrcr-runtime/src/server/dispatch.rs`
- Modify: `crates/ovrcr-runtime/src/server/startup.rs` (state construction)
- Modify: `crates/ovrcr-runtime/src/server/tests.rs` (state construction in `test_state_with_dispatch` and `test_state_with_socket`)
- Create: `src/pi-operator-extension.mjs` (placeholder content in this task; Task 3 fills it) and `src/operator-system-prompt.md`
- Test: `tests/server_lifecycle.rs`

**Interfaces:**
- Consumes: Task 1 types.
- Produces: `ServerState::start_operator(&self) -> Result<SessionSummary>`; `ServerState::dashboard_selection(&self) -> Option<SessionSummary>`; `ServerState::set_dashboard_mode(&self, terminal: bool)`; fields `operator: Mutex<Option<SessionId>>`, `last_user_selection: Mutex<Option<SessionId>>`, `dashboard_terminal_mode: AtomicBool`; `operator::resolve_pi(config: &OperatorConfig) -> Result<PathBuf>`; `operator::operator_dir(registry_path: &Path) -> PathBuf`; `operator::argv(pi: &Path, dir: &Path, config: &OperatorConfig) -> Vec<OsString>`.

- [ ] **Step 1: Write the failing integration tests**

In `tests/server_lifecycle.rs`, next to `register_fixture_workspace`, add a fake Pi helper and five tests. The fake logs its argv and blocks on stdin so the session stays running.

```rust
fn write_fake_pi(dir: &Path) -> PathBuf {
    let script = dir.join("pi");
    std::fs::write(
        &script,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$(dirname \"$0\")/pi-args.log\"\nprintf 'FAKE_PI_READY\\n'\nwhile IFS= read -r line; do [ \"$line\" = quit ] && exit 0; done\n",
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    script
}

fn configure_operator(fixture: &ControlFixture, executable: &Path) {
    let registry_path = fixture._root.path().join("config.toml");
    let mut registry = load_registry(&registry_path).unwrap();
    registry.operator = ovrcr::config::OperatorConfig {
        provider: Some("fixture".into()),
        model: Some("fixture/model".into()),
        thinking: None,
        executable: Some(executable.to_path_buf()),
    };
    save_registry_atomic(&registry, &registry_path).unwrap();
}
```

`ControlFixture` loads the registry once at start, so call `configure_operator` **before** `ControlFixture::new()` in these tests by writing the file into a temp root first; if the fixture's constructor does not allow that, add a `ControlFixture::with_registry(edit: impl FnOnce(&mut Registry))` constructor that applies the edit before spawning the server. The tests below assume that constructor.

```rust
#[test]
fn start_operator_is_idempotent_and_resumes() {
    let _env_lock = ENV_LOCK.lock().unwrap();
    let scripts = tempfile::tempdir().unwrap();
    let pi = write_fake_pi(scripts.path());
    let fixture = ControlFixture::with_registry(|registry| {
        registry.operator.provider = Some("fixture".into());
        registry.operator.model = Some("fixture/model".into());
        registry.operator.executable = Some(pi.clone());
    });
    let first = match fixture.request(Request::StartOperator) {
        Response::CreatedSession(summary) => *summary,
        other => panic!("unexpected response: {other:?}"),
    };
    assert_eq!(first.kind, ovrcr::session::SessionKind::Operator);
    assert_eq!(first.name, "operator");
    let second = match fixture.request(Request::StartOperator) {
        Response::CreatedSession(summary) => *summary,
        other => panic!("unexpected response: {other:?}"),
    };
    assert_eq!(first.id, second.id, "a running operator is returned, not replaced");

    let args = std::fs::read_to_string(scripts.path().join("pi-args.log")).unwrap();
    for expected in [
        "--no-extensions",
        "--extension",
        "--no-context-files",
        "--exclude-tools",
        "edit,write",
        "--session-id",
        "ovrcr-operator",
        "--provider",
        "fixture",
        "--model",
        "fixture/model",
        "--tui-mode",
        "fullscreen",
    ] {
        assert!(args.lines().any(|line| line == expected), "missing {expected} in {args}");
    }

    assert_eq!(
        fixture.request(Request::SendTerminal {
            session: first.id,
            text: "quit".into(),
            submit: true,
        }),
        Response::Ok
    );
    fixture.wait_exited(first.id);
    let third = match fixture.request(Request::StartOperator) {
        Response::CreatedSession(summary) => *summary,
        other => panic!("unexpected response: {other:?}"),
    };
    assert_ne!(third.id, first.id, "an exited operator is replaced");
    match fixture.request(Request::List) {
        Response::Hierarchy(snapshot) => {
            assert_eq!(snapshot.operator.map(|s| s.id), Some(third.id));
        }
        other => panic!("unexpected response: {other:?}"),
    }
    assert_eq!(fixture.request(Request::Shutdown { kill: true }), Response::Ok);
    fixture.join();
}

#[test]
fn operator_never_blocks_removal_or_plain_shutdown() {
    let _env_lock = ENV_LOCK.lock().unwrap();
    let scripts = tempfile::tempdir().unwrap();
    let pi = write_fake_pi(scripts.path());
    let fixture = ControlFixture::with_registry(|registry| {
        registry.operator.executable = Some(pi.clone());
    });
    register_fixture_workspace(&fixture, "feature/operator-guards");
    let local = fixture.only_session_id();
    let operator = match fixture.request(Request::StartOperator) {
        Response::CreatedSession(summary) => *summary,
        other => panic!("unexpected response: {other:?}"),
    };
    let pgid = unsafe { libc::getpgid(operator.pid.unwrap() as libc::pid_t) };
    assert_eq!(fixture.request(Request::CloseTerminal { session: local }), Response::Ok);
    assert_eq!(
        fixture.request(Request::RemoveWorkspace {
            project: "fixture".into(),
            name: "work".into(),
        }),
        Response::Ok
    );
    assert_eq!(
        fixture.request(Request::RemoveProject {
            name: "fixture".into(),
        }),
        Response::Ok
    );
    assert_eq!(fixture.request(Request::Shutdown { kill: false }), Response::Ok);
    fixture.join();
    wait_for_group_absent(pgid, Duration::from_secs(5));
}

#[test]
fn send_to_the_terminal_in_terminal_mode_is_refused() {
    let _env_lock = ENV_LOCK.lock().unwrap();
    let fixture = ControlFixture::new();
    register_fixture_workspace(&fixture, "feature/terminal-mode");
    let local = fixture.only_session_id();
    let mut dashboard = connect_server(&fixture.socket).unwrap();
    dashboard.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    write_frame(&mut dashboard, &ClientMessage { request_id: 1, request: Request::DashboardHello }).unwrap();
    let _ = read_frame::<ServerMessage>(&mut dashboard).unwrap();
    write_frame(
        &mut dashboard,
        &ClientMessage {
            request_id: 2,
            request: Request::Select {
                session: local,
                size: ovrcr::session::TerminalSize { rows: 24, cols: 80 },
            },
        },
    )
    .unwrap();
    let _ = read_frame::<ServerMessage>(&mut dashboard).unwrap();
    write_frame(&mut dashboard, &ClientMessage { request_id: 3, request: Request::DashboardMode { terminal: true } }).unwrap();
    let _ = read_frame::<ServerMessage>(&mut dashboard).unwrap();

    let refused = fixture.request(Request::SendTerminal { session: local, text: "echo hi".into(), submit: true });
    assert!(
        matches!(&refused, Response::Error { code: ErrorCode::Conflict, message } if message.contains("keyboard input")),
        "{refused:?}"
    );

    write_frame(&mut dashboard, &ClientMessage { request_id: 4, request: Request::DashboardMode { terminal: false } }).unwrap();
    let _ = read_frame::<ServerMessage>(&mut dashboard).unwrap();
    assert_eq!(
        fixture.request(Request::SendTerminal { session: local, text: "echo hi".into(), submit: true }),
        Response::Ok
    );
    drop(dashboard);
    assert_eq!(fixture.request(Request::Shutdown { kill: true }), Response::Ok);
    fixture.join();
}

#[test]
fn dashboard_selection_returns_the_previous_user_session() {
    let _env_lock = ENV_LOCK.lock().unwrap();
    let scripts = tempfile::tempdir().unwrap();
    let pi = write_fake_pi(scripts.path());
    let fixture = ControlFixture::with_registry(|registry| {
        registry.operator.executable = Some(pi.clone());
    });
    register_fixture_workspace(&fixture, "feature/selection");
    let local = fixture.only_session_id();
    let operator = match fixture.request(Request::StartOperator) {
        Response::CreatedSession(summary) => *summary,
        other => panic!("unexpected response: {other:?}"),
    };
    assert!(matches!(fixture.request(Request::DashboardSelection), Response::Error { code: ErrorCode::NotFound, .. }));

    let mut dashboard = connect_server(&fixture.socket).unwrap();
    dashboard.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    write_frame(&mut dashboard, &ClientMessage { request_id: 1, request: Request::DashboardHello }).unwrap();
    let _ = read_frame::<ServerMessage>(&mut dashboard).unwrap();
    for (request_id, session) in [(2, local), (3, operator.id)] {
        write_frame(
            &mut dashboard,
            &ClientMessage {
                request_id,
                request: Request::Select {
                    session,
                    size: ovrcr::session::TerminalSize { rows: 24, cols: 80 },
                },
            },
        )
        .unwrap();
        let _ = read_frame::<ServerMessage>(&mut dashboard).unwrap();
    }
    match fixture.request(Request::DashboardSelection) {
        Response::CreatedSession(summary) => assert_eq!(summary.id, local),
        other => panic!("unexpected response: {other:?}"),
    }
    drop(dashboard);
    assert_eq!(fixture.request(Request::Shutdown { kill: true }), Response::Ok);
    fixture.join();
}

#[test]
fn start_operator_reports_missing_pi() {
    let _env_lock = ENV_LOCK.lock().unwrap();
    let fixture = ControlFixture::with_registry(|registry| {
        registry.operator.executable = Some(PathBuf::from("/nonexistent/ovrcr-fake-pi"));
    });
    let response = fixture.request(Request::StartOperator);
    assert!(
        matches!(&response, Response::Error { code: ErrorCode::NotFound, message } if message.contains("/nonexistent/ovrcr-fake-pi") && message.contains("[operator] executable")),
        "{response:?}"
    );
    match fixture.request(Request::List) {
        Response::Hierarchy(snapshot) => assert!(snapshot.operator.is_none()),
        other => panic!("unexpected response: {other:?}"),
    }
    assert_eq!(fixture.request(Request::Shutdown { kill: false }), Response::Ok);
    fixture.join();
}
```

`DashboardSelection` reuses `Response::CreatedSession(Box<SessionSummary>)` as its success shape so no new response variant is needed.

- [ ] **Step 2: Run them to verify they fail**

Run: `rtk proxy cargo test --test server_lifecycle -- start_operator operator_never send_to_the_terminal dashboard_selection`
Expected: compile errors for `with_registry`, then request handling errors once that exists.

- [ ] **Step 3: Add `ControlFixture::with_registry`**

In `tests/server_lifecycle.rs`, refactor `ControlFixture::new` into `with_registry`:

```rust
    fn new() -> Self {
        Self::with_registry(|_| {})
    }

    fn with_registry(edit: impl FnOnce(&mut ovrcr::config::Registry)) -> Self {
        // ... existing body up to `let registry = root.path().join("config.toml");`
        let mut initial = ovrcr::config::Registry::default();
        edit(&mut initial);
        save_registry_atomic(&initial, &registry).unwrap();
        // ... rest of the existing body unchanged
    }
```

- [ ] **Step 4: Create the operator module**

Create `crates/ovrcr-runtime/src/server/operator.rs`:

```rust
//! The reserved operator session: Pi's interactive mode with the OVRCR
//! extension, pinned above the project tree.
use super::*;
use ovrcr_protocol::OperatorConfig;

pub(super) const EXTENSION_SOURCE: &str = include_str!("../../../../src/pi-operator-extension.mjs");
pub(super) const SYSTEM_PROMPT: &str = include_str!("../../../../src/operator-system-prompt.md");

/// Directory holding the extension, prompt, and Pi session files.
pub fn operator_dir(registry_path: &Path) -> PathBuf {
    registry_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .join("operator")
}

/// Resolve the Pi executable: configured path, then OVRCR_PI_EXECUTABLE,
/// then `pi` on PATH. Fails before anything is spawned.
pub fn resolve_pi(config: &OperatorConfig) -> Result<PathBuf> {
    if let Some(configured) = &config.executable {
        if configured.is_file() {
            return Ok(configured.clone());
        }
        return Err(lifecycle_error(
            ErrorCode::NotFound,
            format!(
                "Pi executable {} from [operator] executable does not exist",
                configured.display()
            ),
        ));
    }
    if let Some(from_env) = std::env::var_os("OVRCR_PI_EXECUTABLE") {
        let path = PathBuf::from(from_env);
        if path.is_file() {
            return Ok(path);
        }
        return Err(lifecycle_error(
            ErrorCode::NotFound,
            format!(
                "Pi executable {} from OVRCR_PI_EXECUTABLE does not exist; set [operator] executable in config.toml",
                path.display()
            ),
        ));
    }
    let path_var = std::env::var_os("PATH").unwrap_or_default();
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join("pi");
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(lifecycle_error(
        ErrorCode::NotFound,
        "pi is not on PATH; install Pi or set [operator] executable in config.toml",
    ))
}

/// Write the bundled extension and prompt so an upgraded binary replaces them.
pub fn materialize(dir: &Path) -> Result<(PathBuf, String)> {
    fs::create_dir_all(dir.join("sessions"))
        .with_context(|| format!("create operator directory {}", dir.display()))?;
    let extension = dir.join("ovrcr-operator.mjs");
    fs::write(&extension, EXTENSION_SOURCE)
        .with_context(|| format!("write operator extension {}", extension.display()))?;
    let prompt = dir.join("system-prompt.md");
    fs::write(&prompt, SYSTEM_PROMPT)
        .with_context(|| format!("write operator prompt {}", prompt.display()))?;
    Ok((extension, SYSTEM_PROMPT.to_owned()))
}

pub fn argv(pi: &Path, dir: &Path, extension: &Path, prompt: &str, config: &OperatorConfig) -> Vec<OsString> {
    let mut argv: Vec<OsString> = vec![
        pi.as_os_str().to_owned(),
        "--no-extensions".into(),
        "--extension".into(),
        extension.as_os_str().to_owned(),
        "--no-context-files".into(),
        "--no-skills".into(),
        "--no-prompt-templates".into(),
        "--exclude-tools".into(),
        "edit,write".into(),
        "--system-prompt".into(),
        prompt.into(),
        "--session-dir".into(),
        dir.join("sessions").into_os_string(),
        "--session-id".into(),
        "ovrcr-operator".into(),
        "--name".into(),
        "OVRCR operator".into(),
        "--tui-mode".into(),
        "fullscreen".into(),
    ];
    if let Some(provider) = &config.provider {
        argv.push("--provider".into());
        argv.push(provider.into());
    }
    if let Some(model) = &config.model {
        argv.push("--model".into());
        argv.push(model.into());
    }
    if let Some(thinking) = &config.thinking {
        argv.push("--thinking".into());
        argv.push(thinking.into());
    }
    argv
}
```

Add `mod operator;` in `crates/ovrcr-runtime/src/server/mod.rs` next to the other modules, and `use std::ffi::OsString;` if not already imported there.

Create `src/pi-operator-extension.mjs` with a minimal placeholder that Task 3 replaces:

```js
// Replaced in full by the operator tool set; this stub keeps the binary buildable.
export default function (pi) {}
```

Create `src/operator-system-prompt.md`:

```markdown
You are the OVRCR operator. You manage the terminals and coding agents that OVRCR runs, using only the tools you are given.

Rules:
- Read before you act: list sessions or read a terminal's screen before sending input, closing, or removing anything.
- Identify targets by id and name them back to the user (project / workspace / name, #id) so the user can check you chose the right one.
- Prefer sending commands to a workspace's `local` shell over your own `bash` tool, so the work stays visible in the sidebar.
- Never edit files. You have no edit or write tools; do not work around that with shell redirection.
- Destructive tools ask the user to confirm. If a confirmation is declined, stop and ask what they want instead.
- After acting, report what changed in one or two sentences.
- When a request could mean two different sessions, ask which one before acting.
```

- [ ] **Step 5: Add the server state and `start_operator`**

In `crates/ovrcr-runtime/src/server/mod.rs`, add to `ServerState`:

```rust
    pub operator: Mutex<Option<SessionId>>,
    pub last_user_selection: Mutex<Option<SessionId>>,
    pub dashboard_terminal_mode: AtomicBool,
```

Initialize them (`Mutex::new(None)`, `Mutex::new(None)`, `AtomicBool::new(false)`) at every `ServerState { .. }` construction: `startup.rs` `run_server`, and the two builders in `server/tests.rs`.

Give `create_session_locked` a working-directory override and a kind. Change its signature to

```rust
    fn create_session_locked(
        &self,
        request: ovrcr_protocol::CreateSessionRequest,
        ready: Option<Arc<dyn Fn() + Send + Sync>>,
        placement: SessionPlacement,
    ) -> Result<SessionSummary>
```

with

```rust
enum SessionPlacement {
    Workspace,
    Fixed { cwd: PathBuf, kind: SessionKind },
}
```

Inside, replace the `let (cwd, label) = { registry lookup }` block with:

```rust
        let label = request.label.clone().unwrap_or_else(|| {
            request
                .argv
                .first()
                .and_then(|arg| Path::new(arg).file_name())
                .map(|value| value.to_string_lossy().into_owned())
                .unwrap_or_default()
        });
        let (cwd, kind) = match placement {
            SessionPlacement::Workspace => {
                let registry = self.registry.lock().unwrap();
                let workspace = registry
                    .workspace(&request.project, &request.workspace)
                    .map_err(|error| lifecycle_error(ErrorCode::NotFound, error.to_string()))?;
                (workspace.path.clone(), SessionKind::User)
            }
            SessionPlacement::Fixed { cwd, kind } => (cwd, kind),
        };
```

and pass `kind` into the `SessionSpec`. Every existing caller passes `SessionPlacement::Workspace`.

Add the operator entry point:

```rust
    pub fn start_operator(&self) -> Result<SessionSummary> {
        let _mutation = self.mutation_lock.lock().unwrap();
        self.reject_if_stopping()?;
        if let Some(existing) = *self.operator.lock().unwrap() {
            let session = self.sessions.lock().unwrap().get(&existing).cloned();
            match session {
                Some(session) if !matches!(session.summary().phase, SessionPhase::Exited { .. }) => {
                    return Ok(session.summary());
                }
                Some(_) => self.remove_session_locked(existing)?,
                None => {}
            }
            *self.operator.lock().unwrap() = None;
        }
        let config = self.registry.lock().unwrap().operator.clone();
        let pi = operator::resolve_pi(&config)?;
        let dir = operator::operator_dir(&self.registry_path);
        let (extension, prompt) = operator::materialize(&dir)?;
        let argv = operator::argv(&pi, &dir, &extension, &prompt, &config);
        let summary = self.create_session_locked(
            ovrcr_protocol::CreateSessionRequest {
                project: String::new(),
                workspace: String::new(),
                name: "operator".into(),
                label: Some("ovrcr".into()),
                argv,
            },
            None,
            SessionPlacement::Fixed {
                cwd: dir,
                kind: SessionKind::Operator,
            },
        )?;
        *self.operator.lock().unwrap() = Some(summary.id);
        Ok(summary)
    }

    pub fn dashboard_selection(&self) -> Option<SessionSummary> {
        let id = (*self.last_user_selection.lock().unwrap())?;
        self.sessions.lock().unwrap().get(&id).map(|session| session.summary())
    }

    pub fn set_dashboard_mode(&self, terminal: bool) {
        self.dashboard_terminal_mode.store(terminal, Ordering::Release);
    }
```

`create_session_locked` also needs the child to see `OVRCR_EXECUTABLE`: in `crates/ovrcr-runtime/src/session/mod.rs` `spawn_internal`, after the hook environment lines, add

```rust
        if let Ok(executable) = std::env::current_exe() {
            command.env("OVRCR_EXECUTABLE", executable);
        }
```

(applies to every session; harmless for user sessions and required for the operator).

The duplicate-name check in `create_session_locked` compares project, workspace, and name; the operator's empty project and workspace make `operator` unique.

- [ ] **Step 6: Send refusal, hierarchy, selection tracking, shutdown**

In `send_terminal`, after the exited check:

```rust
        if self.dashboard_terminal_mode.load(Ordering::Acquire)
            && *self.selected.lock().unwrap() == Some(id)
        {
            return Err(lifecycle_error(
                ErrorCode::Conflict,
                "that terminal is receiving keyboard input from the dashboard",
            ));
        }
```

In `snapshot_from_state`, sessions with an empty project never match a workspace, so they are already excluded from the tree; set the new field:

```rust
    let operator = sessions
        .values()
        .map(|session| session.summary())
        .find(|summary| summary.kind == SessionKind::Operator);
    HierarchySnapshot { projects, operator }
```

In `dispatch.rs` `dispatch_select`, right after `*state.selected.lock().unwrap() = Some(id);`:

```rust
    if session.summary().kind == SessionKind::User {
        *state.last_user_selection.lock().unwrap() = Some(id);
    }
```

In `request_shutdown`, before the `if !kill && !sessions.is_empty()` check, close the operator first:

```rust
        let operator_id = *self.operator.lock().unwrap();
        if let Some(id) = operator_id
            && let Some(session) = self.sessions.lock().unwrap().get(&id).cloned()
        {
            session.revoke_hook_capability();
            if let Err(error) = session.terminate(requested_kill_grace()) {
                return error_response(ErrorCode::PartialFailure, format!("operator: {}", error_chain_string(&error)));
            }
            self.sessions.lock().unwrap().remove(&id);
            *self.operator.lock().unwrap() = None;
        }
        let sessions = self.sessions.lock().unwrap().values().cloned().collect::<Vec<_>>();
```

(replace the existing `let sessions = ...` collection with this so the operator is gone before the check).

In `connections.rs` `handle_request_with_id`, add arms:

```rust
        Request::StartOperator => state
            .start_operator()
            .map_or_else(error_for_lifecycle, |summary| {
                dashboard_try_send_arc(
                    state,
                    ServerMessage::Event(ServerEvent::HierarchyChanged(state.hierarchy())),
                );
                Response::CreatedSession(Box::new(summary))
            }),
        Request::DashboardSelection => state.dashboard_selection().map_or_else(
            || error_response(ErrorCode::NotFound, "the dashboard has not selected a session"),
            |summary| Response::CreatedSession(Box::new(summary)),
        ),
        Request::DashboardMode { terminal } => {
            if !dashboard {
                return error_response(ErrorCode::InvalidRequest, "DashboardMode requires a dashboard connection");
            }
            state.set_dashboard_mode(terminal);
            Response::Ok
        }
```

When a dashboard connection closes (the `DashboardOwnership` drop), also reset the mode: add `self.state.dashboard_terminal_mode.store(false, Ordering::Release);` inside the `owns_dashboard` branch of `Drop for DashboardOwnership`.

- [ ] **Step 7: Run the tests**

Run: `rtk proxy cargo test --test server_lifecycle -- start_operator operator_never send_to_the_terminal dashboard_selection`
Expected: 5 passed.

Then: `rtk proxy cargo test --workspace`
Expected: all pass.

- [ ] **Step 8: Commit**

```bash
git add crates/ovrcr-runtime src/pi-operator-extension.mjs src/operator-system-prompt.md tests/server_lifecycle.rs
git commit -m "feat(server): add the reserved operator session and dashboard context requests"
```

---

### Task 3: The Pi extension

**Files:**
- Modify: `src/pi-operator-extension.mjs` (replace the stub)
- Create: `tests/operator_extension.mjs`
- Create: `tests/operator_extension.rs`

**Interfaces:**
- Consumes: `OVRCR_EXECUTABLE`, `OVRCR_SOCKET`, `OVRCR_CONFIG`, `OVRCR_HOOK_SOCKET`, `OVRCR_SESSION_ID`, `OVRCR_HOOK_TOKEN` from the environment; CLI commands `terminal list|read|send|create|close|kill|selected`, `project list|add|remove`, `workspace list|get|create|remove`, `task list|run|pause|resume|delete`, `run list|logs|cancel`, `shutdown`, `report activity`.
- Produces: the tool names in the spec's tables, each returning the CLI's parsed JSON or throwing with the CLI's error message.

- [ ] **Step 1: Write the failing Node contract test**

Create `tests/operator_extension.mjs`:

```js
// Contract test for the operator extension: loads it against a stub Pi API.
// Usage: node tests/operator_extension.mjs <path-to-extension.mjs>
import { strict as assert } from "node:assert";
import { mkdtempSync, writeFileSync, chmodSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

const extensionPath = process.argv[2];
const dir = mkdtempSync(join(tmpdir(), "ovrcr-operator-"));
// A fake ovrcr that echoes its argv as JSON, or fails when asked.
const fake = join(dir, "ovrcr");
writeFileSync(fake, `#!/bin/sh
if [ "$1 $2 $3" = "--json terminal read" ] && [ "$4" = "99" ]; then
  printf '{"error":{"code":"NotFound","message":"session 99 not found"}}\\n' >&2; exit 1
fi
printf '{"argv":["%s"],"socket":"%s"}\\n' "$(printf '%s","' "$@" | sed 's/","$//')" "$OVRCR_SOCKET"
`);
chmodSync(fake, 0o755);
process.env.OVRCR_EXECUTABLE = fake;
process.env.OVRCR_SOCKET = "/tmp/fixture.sock";

const tools = new Map();
const commands = new Map();
const events = new Map();
const pi = {
  registerTool(definition) { tools.set(definition.name, definition); },
  registerCommand(name, options) { commands.set(name, options); },
  on(event, handler) { events.set(event, handler); },
};
const confirmations = [];
let answer = true;
const ctx = { ui: { confirm: async (title, message) => { confirmations.push({ title, message }); return answer; }, notify() {} } };

const { default: register } = await import(pathToFileURL(extensionPath));
await register(pi);

const expected = [
  "dashboard_selection", "list_sessions", "list_projects", "list_workspaces", "read_terminal",
  "workspace_git_status", "list_tasks", "list_runs", "run_logs",
  "create_terminal", "create_workspace", "register_project", "send_terminal", "pause_terminal",
  "resume_terminal", "task_run", "task_pause", "task_resume",
  "close_terminal", "kill_terminal", "remove_workspace", "remove_project", "run_cancel",
  "task_delete", "server_shutdown", "bash",
];
for (const name of expected) assert.ok(tools.has(name), `missing tool ${name}`);
assert.ok(commands.has("status"), "missing /status");

// list_sessions runs the CLI with --json and the inherited environment.
const listed = await tools.get("list_sessions").execute("call-1", {}, ctx);
const listedJson = JSON.parse(listed.content[0].text);
assert.deepEqual(listedJson.argv, ["--json", "terminal", "list"]);
assert.equal(listedJson.socket, "/tmp/fixture.sock");

// A CLI error becomes the tool's error text.
await assert.rejects(
  tools.get("read_terminal").execute("call-2", { id: 99 }, ctx),
  (error) => error.message.includes("session 99 not found"),
);

// close_terminal confirms with the target named and honours a decline.
answer = false;
await assert.rejects(
  tools.get("close_terminal").execute("call-3", { id: 12 }, ctx),
  (error) => error.message === "declined by the user",
);
assert.equal(confirmations.length, 1);
assert.match(confirmations[0].message, /#12/);

// bash confirms before running.
answer = true;
confirmations.length = 0;
const ran = await tools.get("bash").execute("call-4", { command: "echo hi", cwd: dir }, ctx);
assert.equal(confirmations.length, 1);
assert.match(confirmations[0].message, /echo hi/);
assert.match(ran.content[0].text, /hi/);

// send_terminal never confirms.
confirmations.length = 0;
await tools.get("send_terminal").execute("call-5", { id: 3, text: "ls", submit: true }, ctx);
assert.equal(confirmations.length, 0);

console.log("operator extension contract: ok");
```

Create `tests/operator_extension.rs`:

```rust
use std::process::Command;

#[test]
fn operator_extension_contract() {
    let Ok(node) = which_node() else {
        eprintln!("node not on PATH; skipping operator extension contract test");
        return;
    };
    let root = env!("CARGO_MANIFEST_DIR");
    let output = Command::new(node)
        .arg(format!("{root}/tests/operator_extension.mjs"))
        .arg(format!("{root}/src/pi-operator-extension.mjs"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn which_node() -> Result<std::path::PathBuf, ()> {
    let path = std::env::var_os("PATH").ok_or(())?;
    std::env::split_paths(&path)
        .map(|dir| dir.join("node"))
        .find(|candidate| candidate.is_file())
        .ok_or(())
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `rtk proxy cargo test --test operator_extension`
Expected: FAIL with `missing tool dashboard_selection`.

- [ ] **Step 3: Write the extension**

Replace `src/pi-operator-extension.mjs`:

```js
// OVRCR operator extension. Loaded explicitly by the server's operator
// session; discovered extensions stay disabled. Every tool runs the OVRCR
// binary with --json against the inherited OVRCR_SOCKET and OVRCR_CONFIG.
import { execFile } from "node:child_process";
import { promisify } from "node:util";

const run = promisify(execFile);
const ovrcr = process.env.OVRCR_EXECUTABLE || "ovrcr";

async function cli(args) {
  try {
    const { stdout } = await run(ovrcr, ["--json", ...args], { env: process.env, maxBuffer: 8 * 1024 * 1024 });
    return stdout.trim() ? JSON.parse(stdout) : { ok: true };
  } catch (error) {
    let message = error.message;
    try { message = JSON.parse(error.stderr).error.message; } catch {}
    throw new Error(message);
  }
}

function text(value) {
  return { content: [{ type: "text", text: typeof value === "string" ? value : JSON.stringify(value, null, 2) }] };
}

async function describeSession(id) {
  const sessions = await cli(["terminal", "list"]);
  const session = sessions.find((s) => s.id === Number(id));
  return session ? `${session.name} in ${session.project} / ${session.workspace} (#${session.id})` : `#${id}`;
}

async function confirmed(ctx, title, message) {
  const ok = await ctx.ui.confirm(title, message);
  if (!ok) throw new Error("declined by the user");
}

const object = (properties, required = []) => ({ type: "object", properties, required });
const id = { type: "number", description: "terminal id" };
const str = (description) => ({ type: "string", description });

export default function (pi) {
  const tool = (name, description, parameters, execute) => pi.registerTool({ name, description, parameters, execute });

  // Context, no confirmation.
  tool("dashboard_selection", "The session the dashboard last selected, so 'this one' can be resolved.", object({}),
    async () => text(await cli(["terminal", "selected"]).catch(() => "nothing selected")));
  tool("list_sessions", "Every OVRCR session with id, project, workspace, name, phase, agent activity, and context usage.", object({}),
    async () => text(await cli(["terminal", "list"])));
  tool("list_projects", "Registered projects with workspace counts.", object({}), async () => text(await cli(["project", "list"])));
  tool("list_workspaces", "Workspaces of a project with branch and terminal count.", object({ project: str("project name") }, ["project"]),
    async (_id, { project }) => text(await cli(["workspace", "list", "--project", project])));
  tool("read_terminal", "The current screen text of a terminal.", object({ id, max_lines: { type: "number", description: "last N lines" } }, ["id"]),
    async (_id, args) => text(await cli(["terminal", "read", String(args.id), ...(args.max_lines ? ["--max-lines", String(args.max_lines)] : [])])));
  tool("workspace_git_status", "Branch and changed paths of a workspace, read-only.", object({ project: str("project"), workspace: str("workspace") }, ["project", "workspace"]),
    async (_id, { project, workspace }) => {
      const record = await cli(["workspace", "get", "--project", project, "--name", workspace]);
      const { stdout } = await run("git", ["status", "--porcelain", "--branch"], { cwd: record.path, env: process.env });
      return text(stdout);
    });
  tool("list_tasks", "Scheduled tasks.", object({}), async () => text(await cli(["task", "list"])));
  tool("list_runs", "Runs of a task, or all runs.", object({ task: { type: "number" } }),
    async (_id, { task }) => text(await cli(["run", "list", ...(task ? ["--task", String(task)] : [])])));
  tool("run_logs", "A run's transcript.", object({ run: { type: "number" } }, ["run"]),
    async (_id, { run: runId }) => text(await cli(["run", "logs", String(runId)])));

  // Control, no confirmation.
  tool("create_terminal", "Start a terminal in a workspace. Omit command for the user's shell.",
    object({ project: str("project"), workspace: str("workspace"), name: str("session name"), command: str("shell command, run through /bin/sh -lc") }, ["project", "workspace", "name"]),
    async (_id, { project, workspace, name, command }) =>
      text(await cli(["terminal", "create", "--project", project, "--workspace", workspace, "--name", name, ...(command ? ["--", "/bin/sh", "-lc", command] : [])])));
  tool("create_workspace", "Create a worktree workspace. Give base to create a new branch, omit it to use an existing one.",
    object({ project: str("project"), name: str("workspace name"), branch: str("branch"), base: str("base commit-ish for a new branch") }, ["project", "name", "branch"]),
    async (_id, { project, name, branch, base }) =>
      text(await cli(["workspace", "create", "--project", project, "--name", name, ...(base ? ["--new-branch", branch, "--base", base] : ["--branch", branch])])));
  tool("register_project", "Register a Git repository as a project.",
    object({ name: str("project name"), repo: str("repository path"), workspace_root: str("directory for worktrees") }, ["name", "repo", "workspace_root"]),
    async (_id, { name, repo, workspace_root }) => text(await cli(["project", "add", name, repo, "--workspace-root", workspace_root])));
  tool("send_terminal", "Type text into a terminal; submit adds Enter.", object({ id, text: str("text"), submit: { type: "boolean" } }, ["id", "text"]),
    async (_id, args) => text(await cli(["terminal", "send", String(args.id), "--text", args.text, ...(args.submit === false ? ["--no-submit"] : [])])));
  tool("pause_terminal", "SIGSTOP a session's process group.", object({ id }, ["id"]), async (_id, args) => text(await cli(["pause", String(args.id)])));
  tool("resume_terminal", "SIGCONT a paused session.", object({ id }, ["id"]), async (_id, args) => text(await cli(["resume", String(args.id)])));
  tool("task_run", "Run a scheduled task now.", object({ id: { type: "number" } }, ["id"]), async (_id, args) => text(await cli(["task", "run", String(args.id)])));
  tool("task_pause", "Pause a task's schedule.", object({ id: { type: "number" } }, ["id"]), async (_id, args) => text(await cli(["task", "pause", String(args.id)])));
  tool("task_resume", "Resume a task's schedule.", object({ id: { type: "number" } }, ["id"]), async (_id, args) => text(await cli(["task", "resume", String(args.id)])));

  // Destructive, confirmed.
  tool("close_terminal", "Stop a terminal's processes and remove its record. Asks the user first.", object({ id }, ["id"]),
    async (_id, args, ctx) => {
      await confirmed(ctx, "Close terminal", `Close ${await describeSession(args.id)}? Stops its processes and removes the record.`);
      return text(await cli(["terminal", "close", String(args.id)]));
    });
  tool("kill_terminal", "Stop a terminal's processes but keep its final screen. Asks the user first.", object({ id }, ["id"]),
    async (_id, args, ctx) => {
      await confirmed(ctx, "Kill terminal", `Kill ${await describeSession(args.id)}? Stops its processes; the record and final screen remain.`);
      return text(await cli(["terminal", "kill", String(args.id)]));
    });
  tool("remove_workspace", "Remove a clean worktree; keeps the branch. Asks the user first.", object({ project: str("project"), name: str("workspace") }, ["project", "name"]),
    async (_id, { project, name }, ctx) => {
      await confirmed(ctx, "Remove workspace", `Remove workspace ${project} / ${name}? Removes its clean worktree and keeps the branch.`);
      return text(await cli(["workspace", "remove", "--project", project, "--name", name]));
    });
  tool("remove_project", "Unregister a project; keeps the repository. Asks the user first.", object({ name: str("project") }, ["name"]),
    async (_id, { name }, ctx) => {
      await confirmed(ctx, "Remove project", `Unregister project ${name}? The repository stays on disk.`);
      return text(await cli(["project", "remove", name]));
    });
  tool("run_cancel", "Cancel an active task run. Asks the user first.", object({ id: { type: "number" } }, ["id"]),
    async (_id, args, ctx) => {
      await confirmed(ctx, "Cancel run", `Cancel run #${args.id}? Its Pi process is stopped and the run is marked cancelled.`);
      return text(await cli(["run", "cancel", String(args.id)]));
    });
  tool("task_delete", "Delete a task and cancel its pending runs; history is kept. Asks the user first.", object({ id: { type: "number" } }, ["id"]),
    async (_id, args, ctx) => {
      await confirmed(ctx, "Delete task", `Delete task #${args.id}? Pending runs are cancelled; history is kept.`);
      return text(await cli(["task", "delete", String(args.id)]));
    });
  tool("server_shutdown", "Stop the OVRCR server. Asks the user first.", object({ kill: { type: "boolean", description: "terminate every session first" } }),
    async (_id, { kill }, ctx) => {
      await confirmed(ctx, "Shut down OVRCR", kill ? "Terminate every session and stop the server?" : "Stop the server? Refused while sessions remain.");
      return text(await cli(["shutdown", ...(kill ? ["--kill"] : [])]));
    });
  tool("bash", "Run a shell command on the host. Asks the user first. Prefer sending commands to a workspace's local shell.",
    object({ command: str("command"), cwd: str("working directory") }, ["command"]),
    async (_id, { command, cwd }, ctx) => {
      await confirmed(ctx, "Run command", `Run in ${cwd || process.cwd()}:\n${command}`);
      const { stdout, stderr } = await run("/bin/bash", ["-lc", command], { cwd: cwd || process.cwd(), env: process.env, maxBuffer: 8 * 1024 * 1024 });
      return text(stdout + (stderr ? `\n[stderr]\n${stderr}` : ""));
    });

  pi.registerCommand("status", {
    description: "Summarize OVRCR sessions by activity",
    async handler(_args, ctx) {
      const sessions = await cli(["terminal", "list"]);
      const groups = new Map();
      for (const s of sessions) {
        const key = s.phase === "exited" ? "exited" : s.activity || "unknown";
        if (!groups.has(key)) groups.set(key, []);
        groups.get(key).push(`${s.project || "-"} / ${s.workspace || "-"} / ${s.name} (#${s.id}, ${s.phase})`);
      }
      const lines = [...groups].map(([k, v]) => `${k}:\n  ${v.join("\n  ")}`);
      ctx.ui.notify(lines.join("\n") || "no sessions", "info");
    },
  });

  // Presence: the operator's own sidebar spinner.
  const report = (state) => {
    if (!process.env.OVRCR_HOOK_SOCKET) return;
    run(ovrcr, ["report", "activity", "--state", state], { env: process.env }).catch(() => {});
  };
  pi.on("agent_start", () => report("busy"));
  pi.on("agent_end", () => report("idle"));
}
```

If Pi 0.84.4 names its lifecycle events differently, check `dist/core/extensions/types.d.ts` in the installed package for the `on(...)` overloads and use the pair that fires when a turn starts and ends.

- [ ] **Step 4: Run the contract test**

Run: `rtk proxy cargo test --test operator_extension`
Expected: PASS (or a skip message when `node` is absent).

- [ ] **Step 5: Commit**

```bash
git add src/pi-operator-extension.mjs tests/operator_extension.mjs tests/operator_extension.rs
git commit -m "feat(operator): add the Pi extension with confirmed OVRCR tools"
```

---

### Task 4: CLI commands

**Files:**
- Modify: `src/cli/args.rs`
- Modify: `src/cli/mod.rs`
- Modify: `src/cli/resources.rs`
- Modify: `src/cli/output.rs`
- Test: `tests/resource_cli.rs`

**Interfaces:**
- Produces: `ovrcr operator start` (prints the id, `--json` prints the terminal record), `ovrcr operator stop`, `ovrcr terminal selected [--json]`; terminal records and `list --json` carry `"kind": "user" | "operator"`.

- [ ] **Step 1: Write the failing tests**

Append to `tests/resource_cli.rs`:

```rust
#[test]
fn operator_commands_start_stop_and_report_kind() {
    let mut fixture = Fixture::new();
    let pi = fixture.root.path().join("pi");
    std::fs::write(&pi, "#!/bin/sh\nprintf 'FAKE_PI_READY\\n'\nwhile IFS= read -r line; do [ \"$line\" = quit ] && exit 0; done\n").unwrap();
    std::fs::set_permissions(&pi, std::fs::Permissions::from_mode(0o755)).unwrap();
    // The server reads [operator] from config.toml at request time.
    let config = fixture.root.path().join("config.toml");
    let mut registry = ovrcr::config::load_registry(&config).unwrap();
    registry.operator.executable = Some(pi.clone());
    ovrcr::config::save_registry_atomic(&registry, &config).unwrap();
    fixture.ok(&["project", "list"]); // no-op request so the server reloads nothing; the registry is in memory
    let started = fixture.json(&["operator", "start"]);
    assert_eq!(started["kind"], "operator");
    assert_eq!(started["name"], "operator");
    fixture.capture();
    let listed = fixture.json(&["terminal", "list"]);
    assert!(listed.as_array().unwrap().iter().any(|t| t["kind"] == "operator"));
    let selected = fixture.run(&["--json", "terminal", "selected"]);
    assert_eq!(selected.status.code(), Some(1), "no dashboard has selected anything");
    fixture.ok(&["operator", "stop"]);
    let listed = fixture.json(&["terminal", "list"]);
    assert!(listed.as_array().unwrap().iter().all(|t| t["kind"] != "operator"));
}
```

The server holds the registry in memory and only rereads it on start, so the `[operator]` table must be present before the fixture's server starts. Add to `Fixture::new` in `tests/resource_cli.rs` a `config.toml` written before the first `project add` with the `[operator]` table pointing at a fake `pi` created in the fixture root, and drop the in-test rewrite above. Keep the test's assertions.

- [ ] **Step 2: Run it to verify it fails**

Run: `rtk proxy cargo test --test resource_cli -- operator_commands`
Expected: FAIL, `unexpected argument 'operator'`.

- [ ] **Step 3: Add the commands**

In `src/cli/args.rs`, add to `Command`:

```rust
    /// Start or stop the OVRCR operator agent session.
    Operator {
        #[command(subcommand)]
        command: OperatorCommand,
    },
```

```rust
#[derive(Subcommand)]
pub(super) enum OperatorCommand {
    /// Start the operator, or report the running one.
    Start,
    /// Stop the operator and remove its record.
    Stop,
}
```

and to `TerminalCommand`:

```rust
    /// The session the dashboard last selected.
    Selected,
```

In `src/cli/mod.rs`:

```rust
        Command::Operator { command } => match command {
            OperatorCommand::Start => {
                let response = request_without_start(Request::StartOperator)?;
                match response {
                    Response::CreatedSession(summary) => {
                        if json_output {
                            print_json(&terminal_value(&summary, now_unix_ms()))
                        } else {
                            println!("{}", summary.id.0);
                            Ok(())
                        }
                    }
                    other => Err(unexpected_response(other)),
                }
            }
            OperatorCommand::Stop => {
                let listed = request_without_start(Request::List)?;
                let Response::Hierarchy(snapshot) = listed else {
                    return Err(unexpected_response(listed));
                };
                let Some(operator) = snapshot.operator else {
                    return Err(RuntimeError::new(ErrorCode::NotFound, "the operator is not running"));
                };
                mutate_without_start(Request::CloseTerminal { session: operator.id }, json_output)
            }
        },
```

In `src/cli/resources.rs` `run_terminal`, add:

```rust
        TerminalCommand::Selected => match request_without_start(Request::DashboardSelection)? {
            Response::CreatedSession(summary) => {
                if json_output {
                    print_json(&terminal_value(&summary, now_unix_ms()))
                } else {
                    println!("{} {} {} {}", summary.id.0, summary.project, summary.workspace, summary.name);
                    Ok(())
                }
            }
            other => Err(unexpected_response(other)),
        },
```

In `src/cli/output.rs` `terminal_value`, add the field:

```rust
        "kind": match summary.kind {
            SessionKind::User => "user",
            SessionKind::Operator => "operator",
        },
```

and make `terminal list` include the operator: in `run_terminal`'s `List` arm, the sessions come from `inspect()`, which returns every session including the operator, so no change is needed beyond the field. In the human-readable `list` output (`print_legacy_response` or its hierarchy printer), print `operator <id> <name>` on the first line when `snapshot.operator` is set.

- [ ] **Step 4: Run the tests**

Run: `rtk proxy cargo test --test resource_cli --test cli`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add src/cli tests/resource_cli.rs
git commit -m "feat(cli): add operator start/stop, terminal selected, and session kind output"
```

---

### Task 5: Dashboard row, keys, and mode reporting

**Files:**
- Modify: `crates/ovrcr-tui/src/dashboard/mod.rs` (`TreeRow::Operator`)
- Modify: `crates/ovrcr-tui/src/dashboard/state.rs`
- Modify: `crates/ovrcr-tui/src/dashboard/render.rs`
- Modify: `crates/ovrcr-tui/src/dashboard/event_loop.rs` (mode reporting)
- Modify: `crates/ovrcr-tui/src/dashboard/hints.rs` if `plans/2026-09-08-which-key-popup.md` has landed; otherwise skip the popup entry
- Test: `tests/tui.rs`

**Interfaces:**
- Consumes: `HierarchySnapshot.operator`, `Request::StartOperator`, `Request::DashboardMode`, `Response::CreatedSession` with `kind == Operator`.
- Produces: `TreeRow::Operator`; `Dashboard::operator_row_state() -> OperatorRowState { NotStarted, Running, Busy, Exited }`; browse key `o`; `pending_operator_start: Option<u64>`.

- [ ] **Step 1: Write the failing tests**

Append to `tests/tui.rs`:

```rust
fn operator_summary(id: u64, phase: SessionPhase) -> SessionSummary {
    SessionSummary {
        id: SessionId(id),
        project: String::new(),
        workspace: String::new(),
        name: "operator".into(),
        label: "ovrcr".into(),
        pid: Some(4242),
        started_unix_ms: 1_000,
        phase,
        activity: AgentActivity::Idle,
        context_usage: None,
        kind: ovrcr::session::SessionKind::Operator,
    }
}

#[test]
fn operator_row_is_first_and_shows_not_started() {
    let dashboard = dashboard_fixture();
    let rows = dashboard.visible_rows();
    assert_eq!(rows[0], ovrcr::tui::TreeRow::Operator);
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal.draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 100)).unwrap();
    let text = buffer_text(terminal.backend().buffer());
    assert!(text.contains("OVRCR operator"));
    assert!(text.contains("not started"));
}

#[test]
fn enter_on_operator_row_starts_it_then_attaches() {
    let mut dashboard = dashboard_fixture();
    dashboard.select_operator_row();
    let action = dashboard.key(KeyCode::Enter);
    let ovrcr::tui::DashboardAction::Request(message) = action else {
        panic!("expected a StartOperator request, got {action:?}");
    };
    assert_eq!(message.request, Request::StartOperator);
    let started = operator_summary(77, SessionPhase::Running);
    let outgoing = dashboard.handle_server_message(ServerMessage::Response {
        request_id: message.request_id,
        response: Response::CreatedSession(Box::new(started.clone())),
    });
    assert!(matches!(outgoing.first().map(|m| &m.request), Some(Request::Select { session, .. }) if *session == SessionId(77)));
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(HierarchySnapshot {
        projects: dashboard.hierarchy.projects.clone(),
        operator: Some(started),
    })));
    assert_eq!(dashboard.selected, Some(SessionId(77)));
    assert_eq!(dashboard.mode, InputMode::Terminal);
}

#[test]
fn o_jumps_to_a_running_operator_and_remembers_the_previous_selection() {
    let mut dashboard = dashboard_fixture();
    let previous = dashboard.selected.unwrap();
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(HierarchySnapshot {
        projects: dashboard.hierarchy.projects.clone(),
        operator: Some(operator_summary(77, SessionPhase::Running)),
    })));
    let action = dashboard.key(KeyCode::Char('o'));
    assert!(matches!(action, ovrcr::tui::DashboardAction::Request(ref m) if matches!(m.request, Request::Select { session, .. } if session == SessionId(77))));
    assert_eq!(dashboard.mode, InputMode::Terminal);
    assert_eq!(dashboard.previous_user_selection, Some(previous));
}

#[test]
fn operator_row_shows_exited_and_spinner_states() {
    let mut dashboard = dashboard_fixture();
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::HierarchyChanged(HierarchySnapshot {
        projects: dashboard.hierarchy.projects.clone(),
        operator: Some(operator_summary(77, SessionPhase::Exited { code: Some(0), signal: None })),
    })));
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal.draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 100)).unwrap();
    assert!(buffer_text(terminal.backend().buffer()).contains("exited"));
    let mut busy = operator_summary(77, SessionPhase::Running);
    busy.activity = AgentActivity::Busy;
    dashboard.handle_server_message(ServerMessage::Event(ServerEvent::SessionChanged(Box::new(busy))));
    terminal.draw(|frame| ovrcr::tui::draw_dashboard_at(frame, &dashboard, 100)).unwrap();
    assert!(!buffer_text(terminal.backend().buffer()).contains("exited"));
}

#[test]
fn entering_and_leaving_terminal_mode_reports_dashboard_mode() {
    let mut dashboard = dashboard_fixture();
    let entered = dashboard.key(KeyCode::Enter);
    assert!(matches!(entered, ovrcr::tui::DashboardAction::Request(ref m) if m.request == Request::DashboardMode { terminal: true }));
    let left = dashboard.ctrl('g');
    assert!(matches!(left, ovrcr::tui::DashboardAction::Request(ref m) if m.request == Request::DashboardMode { terminal: false }));
}
```

`buffer_text` is the existing helper in `tests/tui.rs` that flattens a `TestBackend` buffer; if it has a different name there, use that one. `dashboard.key` and `dashboard.ctrl` are the existing test helpers.

- [ ] **Step 2: Run them to verify they fail**

Run: `rtk proxy cargo test --test tui -- operator_ enter_on_operator o_jumps entering_and_leaving`
Expected: compile errors (`TreeRow::Operator`, `select_operator_row`, `previous_user_selection`).

- [ ] **Step 3: Implement the row and keys**

In `crates/ovrcr-tui/src/dashboard/mod.rs`, add `Operator,` as the first variant of `TreeRow`.

In `state.rs`:
- Add fields to `Dashboard`: `pub previous_user_selection: Option<SessionId>`, `pending_operator_start: Option<u64>`, `operator_row_selected: bool`.
- `visible_rows`: push `TreeRow::Operator` first, before iterating projects.
- `move_selection`: treat the operator row as selectable: build `ids` from `TreeRow::Session { id }` plus, when `self.hierarchy.operator` is `Some(summary)`, its id at index 0; when it is `None`, `j`/`k` onto the row sets `operator_row_selected = true` and `selected = None`.
- Add `pub fn select_operator_row(&mut self)` that sets `operator_row_selected = true`, clears `selected`, and returns.
- Add `pub fn operator_row_state(&self) -> OperatorRowState` mapping `hierarchy.operator`: `None` → `NotStarted`; `Exited` → `Exited`; `activity == Busy` → `Busy`; else `Running`.
- In `key_action`, browse mode, before the existing `KeyCode::Enter` arm:

```rust
                    KeyCode::Enter if self.operator_row_selected && self.hierarchy.operator.is_none() => {
                        let request_id = self.next_request_id();
                        self.pending_operator_start = Some(request_id);
                        DashboardAction::Request(ClientMessage { request_id, request: Request::StartOperator })
                    }
                    KeyCode::Char('o') => self.jump_to_operator(),
```

with

```rust
    fn jump_to_operator(&mut self) -> DashboardAction {
        match self.hierarchy.operator.as_ref().map(|s| (s.id, s.phase.clone())) {
            Some((id, SessionPhase::Exited { .. })) | None => {
                self.select_operator_row();
                let request_id = self.next_request_id();
                self.pending_operator_start = Some(request_id);
                DashboardAction::Request(ClientMessage { request_id, request: Request::StartOperator })
            }
            Some((id, _)) => {
                if let Some(current) = self.selected.filter(|s| Some(*s) != self.hierarchy.operator.as_ref().map(|o| o.id)) {
                    self.previous_user_selection = Some(current);
                }
                self.select_session(id);
                self.mode = InputMode::Terminal;
                let request_id = self.next_request_id();
                DashboardAction::Request(self.select_request(id, request_id))
            }
        }
    }
```

- In `handle_server_message`, `Response::CreatedSession` arm: when `self.pending_operator_start == Some(request_id)`, clear it, set `previous_user_selection` from the current non-operator selection, call `select_session(session.id)`, set `mode = InputMode::Terminal`, `operator_row_selected = false`, and push the select request to `outgoing`. When the response is an error for that request id, set `self.error` and clear `pending_operator_start`.
- Mode reporting: every place that sets `self.mode = InputMode::Terminal` or leaves it (`Ctrl-g`, `EnterBrowse`, palette open, history and copy exits) goes through a new `fn set_mode(&mut self, mode: InputMode) -> Option<ClientMessage>` that returns `Some(DashboardMode { terminal })` when the terminal-ness changed. In `key_action`, the Enter arm becomes `self.set_mode(InputMode::Terminal).map_or(DashboardAction::Redraw, DashboardAction::Request)`, and the Ctrl-g arm returns `DashboardAction::Request(...)` when a message is produced, else `EnterBrowse`. The event loop already writes any `DashboardAction::Request`; check that `EnterBrowse` handling in `event_loop.rs` still re-enables mouse capture when the mode leaves terminal, and add a second write for the mode message there if the action type cannot carry both (add `DashboardAction::RequestThenBrowse(ClientMessage)` if needed).

In `render.rs` `tree_row_text`, add the arm:

```rust
        TreeRow::Operator => {
            let (suffix, style) = match dashboard.operator_row_state() {
                OperatorRowState::NotStarted => ("  not started", base_style.fg(MUTED)),
                OperatorRowState::Exited => ("  exited", base_style.fg(MUTED)),
                OperatorRowState::Busy => ("  ⠋", base_style.fg(MAUVE).add_modifier(Modifier::BOLD)),
                OperatorRowState::Running => ("", base_style.fg(MAUVE).add_modifier(Modifier::BOLD)),
            };
            let style = if dashboard.operator_row_selected || dashboard.selected == dashboard.hierarchy.operator.as_ref().map(|o| o.id) { style.bg(CRUST).fg(TEXT) } else { style };
            (clip_text(&format!("󱚝 OVRCR operator{suffix}"), width), style)
        }
```

Use the same spinner frames the session rows already use for `Busy` rather than a fixed glyph; look at how `tree_row_text` animates `AgentActivity::Busy` and reuse that call. Also in `render.rs`, the row height function that maps `TreeRow` to line counts returns `1` for `Operator`, and the metadata line above the pane shows "OVRCR operator" as the title when the operator is selected.

If `hints.rs` exists, add to the View group: key `o`, name "Operator", description "Talk to the OVRCR operator about your sessions".

- [ ] **Step 4: Run the tests**

Run: `rtk proxy cargo test --test tui` and `rtk proxy cargo test -p ovrcr-tui`
Expected: all pass, including the existing selection and rendering tests (the operator row shifts every row index by one; update fixtures that assert absolute row positions).

- [ ] **Step 5: Commit**

```bash
git add crates/ovrcr-tui tests/tui.rs
git commit -m "feat(dashboard): pin the operator row, add o and Enter-to-start, report terminal mode"
```

---

### Task 6: Acceptance test and documentation

**Files:**
- Modify: `tests/terminal_acceptance.rs`
- Modify: `README.md`
- Modify: `docs/testing-computer-use.md`

- [ ] **Step 1: Write the failing acceptance test**

Append to `tests/terminal_acceptance.rs`, reusing `AcceptanceFixture` and `OuterDashboard`:

```rust
#[test]
fn operator_starts_from_the_dashboard_and_survives_reattach() -> Result<()> {
    let mut fixture = AcceptanceFixture::new()?;
    let pi = fixture.root_path().join("pi");
    std::fs::write(&pi, "#!/bin/sh\nprintf 'FAKE_PI_READY\\n'\nwhile IFS= read -r line; do printf 'OPERATOR_GOT_%s\\n' \"$line\"; done\n")?;
    std::fs::set_permissions(&pi, std::fs::Permissions::from_mode(0o755))?;
    fixture.set_operator_executable(&pi)?;
    fixture.setup()?;
    let mut dashboard = OuterDashboard::start(&fixture, PtySize { rows: 30, cols: 100, pixel_width: 0, pixel_height: 0 })?;
    dashboard.wait_for(b"OVRCR operator", Duration::from_secs(3))?;
    dashboard.wait_for(b"not started", Duration::from_secs(3))?;
    dashboard.send(b"o")?;
    dashboard.wait_for(b"FAKE_PI_READY", Duration::from_secs(5))?;
    dashboard.send(b"hello operator\r")?;
    dashboard.wait_for(b"OPERATOR_GOT_hello operator", Duration::from_secs(3))?;
    dashboard.send(b"\x07")?;
    dashboard.wait_for(b"BROWSE", Duration::from_secs(3))?;
    dashboard.detach()?;
    let mut reattached = OuterDashboard::start(&fixture, PtySize { rows: 30, cols: 100, pixel_width: 0, pixel_height: 0 })?;
    reattached.wait_for(b"OPERATOR_GOT_hello operator", Duration::from_secs(5))?;
    reattached.detach()?;
    fixture.shutdown()?;
    Ok(())
}
```

Add to `AcceptanceFixture`: `fn root_path(&self) -> &Path` returning the temp root, and `fn set_operator_executable(&self, pi: &Path) -> Result<()>` that loads the fixture's `config.toml`, sets `registry.operator.executable`, and saves it. The fixture starts its server in `new()`; if the server has already loaded the registry by the time `set_operator_executable` runs, move the call before the server spawn by giving `AcceptanceFixture::new` an `operator: Option<PathBuf>` parameter used when writing the initial registry. `fixture.shutdown()` uses `shutdown --kill`, which includes the operator.

- [ ] **Step 2: Run it to verify it fails**

Run: `rtk proxy cargo test --test terminal_acceptance -- operator_starts`
Expected: FAIL on the missing helper, then on "not started" not rendering until Task 5's row exists (it does by now).

- [ ] **Step 3: Make it pass**

Add the two fixture helpers described above. Run: `rtk proxy cargo test --test terminal_acceptance`. Expected: all pass.

- [ ] **Step 4: Document**

In `README.md`, add a section `## The OVRCR operator` after "Command palette":

```markdown
## The OVRCR operator

The first row of the sidebar is the OVRCR operator, an agent that manages
your terminals and other agents. Press `o` from browse mode, or select the
row and press Enter, to start it; the first start writes its Pi session
under the configuration directory and later starts resume that conversation.
It runs Pi's interactive mode with only OVRCR's tools plus `read` and
`bash`, so it can list sessions, read a terminal's screen, start terminals
and workspaces, send input, pause and resume, and inspect scheduled runs.
Closing or killing a terminal, removing a workspace or project, cancelling
a run, deleting a task, shutting the server down, and running `bash` each
ask you to confirm inside the pane first. It never types into the terminal
the dashboard currently has in terminal mode.

Ask it things like "what is waiting for input", "start codex in cleanup and
ask it to run the tests", or "close every idle shell in consigint". `/status`
inside the pane summarizes sessions by activity. Ctrl-g returns to browse
and `q` detaches; the operator keeps running like any session.

Configure the model in `config.toml`:

```toml
[operator]
provider = "anthropic"          # omitted: Pi's default provider
model = "claude-sonnet-4-5"     # omitted: Pi's default model
thinking = "low"                # omitted: Pi's default
executable = "/opt/pi/bin/pi"   # omitted: OVRCR_PI_EXECUTABLE, then PATH
```

Credentials stay in Pi's own configuration. `ovrcr operator start`,
`ovrcr operator stop`, and `ovrcr terminal selected` expose the same
operations to scripts. A missing Pi or model shows its error in the pane;
the row then reads "exited" and the next `o` starts a fresh process.
```

In `docs/testing-computer-use.md`, add a step 3b: with a real Pi and a configured model, press `o`, wait for the prompt, ask "what is waiting for input", and confirm the answer names the demo session that printed a prompt; then close the pane with Ctrl-g and confirm the operator row still shows as running.

- [ ] **Step 5: Run the full gate and commit**

Run: `rtk proxy cargo fmt --all -- --check`, `rtk proxy cargo clippy --all-targets --all-features -- -D warnings`, `rtk proxy cargo test --workspace`
Expected: all pass.

```bash
git add tests/terminal_acceptance.rs README.md docs/testing-computer-use.md
git commit -m "test(operator): acceptance coverage and documentation"
```

---

## Self-review notes

- Spec coverage: session kind, `HierarchySnapshot.operator`, `StartOperator` idempotence and exited replacement, executable resolution order, materialized extension and prompt, argv, hook environment and `OVRCR_EXECUTABLE`, removal and shutdown guards, `DashboardSelection` and `DashboardMode` with the refusal message, CLI commands, the tool tables with confirmation rules, `/status`, presence, the row states, `o` and Enter, mode reporting, configuration table, failure handling, and every test in the spec's testing section map to Tasks 1 through 6.
- One judgment call: `DashboardSelection` reuses `Response::CreatedSession` as its success shape to avoid another response variant. If reviewers prefer a dedicated `Response::Session(Box<SessionSummary>)`, add it in Task 1 and use it in Tasks 2, 4, and 5.
- The row spinner glyph in Task 5 is a placeholder for whichever animation the session rows already use; the implementer must reuse that function rather than a fixed glyph.

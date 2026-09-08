# OVRCR Operator Agent Design

## Purpose

OVRCR gets a native agent that manages and operates its terminals and agents. It appears as a pinned entry at the top of the dashboard sidebar, behaves like any other terminal session, and runs Pi's interactive mode inside a PTY with an OVRCR-specific extension. The user selects it, types requests such as "start codex in cleanup and ask it to run the tests" or "what is waiting for input", and the operator acts through OVRCR's own control commands.

## Decisions

These were settled during design and are not open:

- **Interactive Pi in a PTY, not RPC.** Pi's interactive mode accepts every option the operator needs, and its extension API provides confirmation dialogs, slash commands, and widgets. The RPC driver stays with scheduled tasks.
- **Full control with confirmation on destructive actions.** Create, send, pause, resume, and scheduler run and pause need no confirmation. Close, kill, remove, cancel, delete, shutdown, and `bash` show a confirm dialog naming the target and the consequence.
- **Never type into the terminal the user is typing into.** A send aimed at the session the dashboard has in terminal mode is refused by the server, whoever sends it.
- **Lazy start.** The pinned entry always shows; the operator process starts on the first Enter and restarts on demand after it exits, resuming its saved conversation.
- **Tools: OVRCR control plus Pi's `read` and `bash`,** with `bash` behind confirmation. `edit` and `write` are excluded.
- **Model from an `[operator]` table** in the server's `config.toml` with `provider`, `model`, `thinking`, and `executable`, each falling back to Pi's own default.
- **Context tools,** including the dashboard's last selection, so "this one" resolves.

## Architecture

A reserved session kind owned by the server. One request, `StartOperator`, spawns a PTY session with a fixed argv and marks its summary `kind: Operator`. Every other mechanism is the existing session machinery: PTY ownership, output and history capture, pause, kill, detach, and reattach. The dashboard pins any summary of that kind at the top of the tree. The extension inside Pi implements tools by running the OVRCR binary with `--json`, so the operator uses the same versioned control surface as scripts and the CLI.

```
dashboard ──Select/Input──▶ server ──PTY──▶ pi (interactive)
    ▲                         ▲                │ --extension ovrcr-operator.mjs
    │ HierarchyChanged        │                │
    └─────────────────────────┘                ▼
                        ovrcr --json ... ◀── tools (child processes)
```

## Server

**Session kind.** `SessionSummary` gains `kind: SessionKind` with variants `User` and `Operator`. `HierarchySnapshot` gains `operator: Option<SessionSummary>` so clients receive the operator outside the project tree. Together with the new requests this is one `PROTOCOL_VERSION` bump and a regenerated wire snapshot.

**State.** `ServerState.operator: Mutex<Option<SessionId>>` tracks the single operator. `Request::StartOperator` is idempotent:

1. If the tracked operator is `Running` or `Paused`, return its summary.
2. If it has exited, remove its record, then continue.
3. Resolve the Pi executable: `[operator] executable`, then `OVRCR_PI_EXECUTABLE`, then `PATH`. Fail with an error naming what was searched if none exists; nothing is spawned.
4. Create `<data dir>/operator` if missing, write the bundled extension and system prompt there, and fail with the path if that is not possible.
5. Spawn through `create_session_locked` with `project` and `workspace` empty, `name` `operator`, `label` `ovrcr`, `kind` `Operator`, and the argv below. The spawn path takes an explicit working directory, `<data dir>/operator`, instead of resolving one from the registry, since the operator has no workspace. Store the id and return `CreatedSession`.

**Argv.**

```
<pi> --no-extensions --extension <dir>/ovrcr-operator.mjs
     --no-context-files --no-skills --no-prompt-templates
     --exclude-tools edit,write
     --system-prompt <contents of <dir>/system-prompt.md>
     --session-dir <dir>/sessions --session-id ovrcr-operator
     --name "OVRCR operator" --tui-mode fullscreen
     [--provider <provider>] [--model <model>] [--thinking <level>]
```

`--session-id` creates the Pi session when missing and resumes it otherwise. The child inherits `OVRCR_SOCKET` and `OVRCR_CONFIG`, receives `OVRCR_EXECUTABLE` set to the server's own executable path, and gets the same hook environment as any session so its activity reports work.

**Guards.** The operator belongs to no project, so workspace and project removal never see it. `Shutdown { kill: false }` closes the operator with the ordinary termination path before checking that no user sessions remain; `Shutdown { kill: true }` includes it with everything else. `terminal list` and `list --json` include it with its kind.

**Context requests.**
- `Request::DashboardSelection` returns the summary of the session the dashboard last selected other than the operator, or `NotFound` when there is none. The server tracks this itself: whenever a `Select` changes the dashboard's selection to a `User` session, that id is remembered as the last user selection, so selecting the operator does not disturb it. No change to the `Select` request is needed.
- `Request::DashboardMode { terminal: bool }` records whether the dashboard is in terminal mode. `SendTerminal` to the dashboard's selected session while `terminal` is true returns `Conflict` with the message "that terminal is receiving keyboard input from the dashboard".

**CLI.** `ovrcr operator start` and `ovrcr operator stop` map to `StartOperator` and `CloseTerminal`. `ovrcr terminal selected --json` maps to `DashboardSelection`. Neither start a server; `operator start` requires a running one, since the operator only makes sense alongside the dashboard.

## Extension and tools

**File.** `src/pi-operator-extension.mjs`, embedded with `include_str!` beside the task extension, plus `src/operator-system-prompt.md`. Both are written to `<data dir>/operator` on every start so an upgraded binary replaces them.

**Execution.** Every tool runs `OVRCR_EXECUTABLE --json <args>` with the inherited environment, captures stdout and stderr, and returns parsed JSON as the tool result. A non-zero exit returns the CLI's JSON error `message` as the tool error. The CLI's request bound, 30 seconds or 60 for kill and close, bounds each tool.

**Context tools, no confirmation.**

| Tool | CLI call | Returns |
| --- | --- | --- |
| `dashboard_selection` | `terminal selected` | the previously selected session, or "nothing selected" |
| `list_sessions` | `terminal list` | every session with id, project, workspace, name, label, phase, activity, context usage, PID, and kind |
| `list_projects` | `project list` | projects with workspace counts |
| `list_workspaces(project)` | `workspace list --project` | workspaces with branch and terminal count |
| `read_terminal(id, max_lines)` | `terminal read` | current screen text |
| `workspace_git_status(project, workspace)` | `workspace get` then `git status --porcelain --branch` in its path | branch and changed paths; read-only |
| `list_tasks`, `list_runs(task)`, `run_logs(run, max_bytes)` | `task list`, `run list`, `run logs` | scheduler records and transcripts |

**Control tools, no confirmation.** `create_terminal(project, workspace, name, command)`, `create_workspace(project, name, branch, base)`, `register_project(name, repo, workspace_root)`, `send_terminal(id, text, submit)`, `pause_terminal(id)`, `resume_terminal(id)`, `task_run(id)`, `task_pause(id)`, `task_resume(id)`. A refused send surfaces the server's message unchanged.

**Confirmed tools.** `close_terminal(id)`, `kill_terminal(id)`, `remove_workspace(project, name)`, `remove_project(name)`, `run_cancel(id)`, `task_delete(id)`, `server_shutdown(kill)`, `bash(command, cwd)`. Each first resolves the target's name through a context tool, then calls `ctx.ui.confirm` with a title naming the action and a message naming the target and consequence, for example "Close codex in consigint / cleanup (#12)? Stops its processes and removes the record." A decline returns the tool error "declined by the user"; nothing is retried automatically. `bash` shows the command and directory in the dialog.

**Presence.** On Pi's agent-start and agent-end events the extension runs `OVRCR_EXECUTABLE report activity --state busy` and `--state idle` with the inherited hook environment, so the operator's sidebar row shows the same spinner as other agents.

**Slash command.** `/status` prints sessions grouped by activity, each with project, workspace, name, id, and phase.

**System prompt.** The operator manages OVRCR sessions and agents. It reads before it acts, identifies targets by id and names them back to the user, prefers sending commands to a workspace's `local` shell over its own `bash` so work stays visible in the sidebar, never edits files, reports what changed after acting, and asks when a request is ambiguous between two sessions.

## Dashboard

**Row.** `TreeRow::Operator` is always the first row in `visible_rows`, above every project, in its own style with the label "OVRCR operator" and a state suffix: nothing while running, the activity spinner while busy, "not started" before the first start, "exited" after Pi quits. It is not collapsible. `j` and `k` reach it first; a mouse click selects it.

**Starting.** Enter on the row with no running operator sends `StartOperator`, selects the returned session, and enters terminal mode. With a running operator, Enter attaches. A failed start shows the error in the footer and leaves the row "not started". The browse-mode key `o` jumps to the operator from any row and attaches; it appears in the which-key popup under View with the description "Talk to the OVRCR operator about your sessions".

**Unchanged behavior.** The pane is the existing terminal renderer. Ctrl-g returns to browse, `q` detaches and leaves the operator running, history and copy mode work on it, the palette's "Close terminal" closes it with the usual confirmation, and reattach restores whatever was selected.

**Reporting.** The dashboard sends `DashboardMode` on every transition into and out of terminal mode. It needs no extra bookkeeping for the previous selection, since the server derives that from the sequence of `Select` requests. A `SessionChanged` for the operator updates its row like any session.

## Configuration

```toml
[operator]
provider = "anthropic"          # omitted: Pi's default provider
model = "claude-sonnet-4-5"     # omitted: Pi's default model
thinking = "low"                # omitted: Pi's default
executable = "/opt/pi/bin/pi"   # omitted: OVRCR_PI_EXECUTABLE, then PATH
```

`Registry` gains the table with `#[serde(default)]`, so existing files load unchanged and the server's atomic rewrite preserves it. Credentials stay in Pi's own auth configuration or the environment file the installed service loads; nothing secret goes in `config.toml`.

## Lifecycle and failure handling

- The operator lives in the server's session map and dies with the server. Its conversation persists in Pi's session file under `<data dir>/operator/sessions` and resumes on the next start.
- Pi exiting for any reason leaves an exited record with its final screen; the next start replaces that record.
- Pi not found: `StartOperator` fails before spawning, naming the executable searched and the `[operator] executable` hint.
- Model or authentication missing: Pi prints its error in the pane and exits; the row shows "exited" and the screen shows why.
- Operator directory or extension not writable: `StartOperator` fails with the path.
- Tool CLI failure or timeout: the tool result carries the error text; the model continues.
- Confirm declined: reported as a tool error, never retried.
- Binary upgraded under a running server: the protocol handshake reports the version mismatch inside the tool result, the same message the CLI shows.

## Testing

**Server**, in `tests/server_lifecycle.rs`, with a fake `pi` script on `PATH` that logs its arguments and waits on stdin:
- `start_operator_is_idempotent_and_resumes`: two requests return one id; the argument log shows `--session-id ovrcr-operator`, the extension path, `--exclude-tools edit,write`, and the configured provider and model; after the fake exits, a third request yields a new id.
- `operator_never_blocks_removal_or_plain_shutdown`: with the operator running, `RemoveWorkspace`, `RemoveProject`, and `Shutdown { kill: false }` succeed and the operator's process group is gone afterwards.
- `send_to_the_terminal_in_terminal_mode_is_refused`: after `DashboardMode { terminal: true }` on a selected session, `SendTerminal` to it returns `Conflict`; after `terminal: false` it succeeds.
- `dashboard_selection_returns_the_previous_session`: select A, then the operator; the request returns A.
- `start_operator_reports_missing_pi`: an empty `PATH` and no configured executable yield an error before any process exists.

**Extension**, in `tests/operator_extension.mjs`, run from a Rust test only when `node` is on `PATH`, against a stub `pi` object recording `registerTool` calls and a stub `ctx.ui.confirm`: the tool list matches this spec; `close_terminal` confirms with the target named and returns "declined by the user" when refused; `bash` confirms; `list_sessions` runs `OVRCR_EXECUTABLE --json terminal list` with the inherited environment; a CLI error becomes the tool's error text.

**Dashboard**, in `tests/tui.rs`: the operator row renders first with "not started"; Enter emits `StartOperator`, then selects the created session and enters terminal mode; `o` from a session row switches and remembers the previous selection; the which-key popup lists `o`; a `SessionChanged` for the operator updates the spinner.

**Acceptance**, in `tests/terminal_acceptance.rs` with the fake `pi`: start the operator through the real dashboard in a PTY, type a line and confirm it reaches the fake's stdin, then detach, reattach, and confirm the pane restores.

**Manual**, in `docs/testing-computer-use.md`: with a real Pi and model, press `o`, ask "what is waiting for input", and confirm the answer names the demo session.

## Out of scope

- Eager start and a watching role that acts without a request. Lazy start leaves room for an `[operator] autostart` flag later.
- Multiple operators or per-project operators.
- OVRCR rendering Pi's transcript itself; Pi's own TUI is the interface.
- Conversation summarization; Pi's own session tooling handles context.
- Storing credentials in OVRCR configuration.

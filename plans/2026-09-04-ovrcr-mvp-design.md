# OVRCR MVP Design

**Date:** 2026-09-04  
**Status:** Approved design

## Purpose

OVRCR is a terminal multiplexer for one user managing many CLI coding-agent sessions. It groups sessions by Git project and worktree, keeps processes alive when the dashboard disconnects, and presents one selected terminal beside a project tree.

The MVP targets macOS and Linux, supports up to 50 live sessions, and assumes one connected dashboard. It favors a small synchronous implementation over an async runtime.

## Product boundary

OVRCR owns every session it manages. It cannot adopt a process launched through another terminal or PTY.

The MVP provides:

- A persistent per-user server that owns PTYs and child processes.
- A Ratatui dashboard modeled on the approved OVRCR mockup.
- CLI commands for projects, Git worktrees, and sessions.
- Detach and reattach while the server remains alive.
- Current-screen reconstruction for background and reattached sessions.
- Safe Git worktree creation and removal.

The MVP does not preserve sessions across a server crash or machine reboot.

## Terminology

- **Project:** A registered Git repository and the root under which OVRCR may create its worktrees.
- **Workspace:** A Git worktree created and recorded by OVRCR.
- **Session:** A command launched by OVRCR inside a workspace-owned PTY.
- **Reattach:** Connect a dashboard to a PTY that is still owned by the running OVRCR server.
- **Restore:** Recreate or resume work after the original process or PTY is gone. Restore is deferred.

## Architecture

OVRCR ships as one Rust executable with three runtime roles:

```text
                    Unix socket
  control command -------------------+
  control command -------------------+---> OVRCR server ---> PTYs and children
                                      |
  terminal ---> OVRCR dashboard -----+
```

The commands and dashboard are clients of the same server. The dashboard and mutating commands start the server automatically when they cannot connect to it. `ovrcr list` reports an empty runtime when no server exists, and `ovrcr shutdown` reports that there is nothing to stop. A bind race decides concurrent startup attempts: the process that binds the socket becomes the server and the other process reconnects.

The launcher starts the server in a new Unix session with standard input disconnected. The server therefore survives the terminal that launched it.

On Linux, the socket lives beneath `$XDG_RUNTIME_DIR` when available and otherwise beneath a user-specific directory in the system temporary directory. On macOS, it lives beneath a user-specific directory in `$TMPDIR`. OVRCR creates its socket directory with mode `0700`. Only one dashboard may remain connected. Short-lived control clients may connect while the dashboard is open.

The server stores live sessions in memory. It serializes project, workspace, session, and shutdown mutations so two clients cannot race the same operation.

### Source layout

```text
src/
  main.rs       CLI parsing and command dispatch
  config.rs     project and workspace registry
  git.rs        guarded Git worktree operations
  protocol.rs   framed Unix-socket messages
  server.rs     daemon and in-memory session registry
  session.rs    PTY, process group, and terminal state
  tui.rs        dashboard rendering and input modes
```

This is one Cargo package rather than a multi-crate workspace.

## Dependencies

- `clap` parses the CLI.
- `ratatui` and `crossterm` render the dashboard and read terminal events.
- `portable-pty` creates and resizes PTYs on macOS and Linux.
- `vt100` parses terminal byte streams into screen state.
- `serde` and `toml` encode the small persistent registry.
- A compact length-prefixed serializer encodes local socket messages.
- `libc` supplies Unix process-group signaling.

OVRCR does not use Tokio, a database, a shell-command builder, or an agent SDK.

## Persistent registry

OVRCR stores registered projects and OVRCR-owned workspaces in a small user config file. The default is `$XDG_CONFIG_HOME/ovrcr/config.toml`, or `~/.config/ovrcr/config.toml` when `XDG_CONFIG_HOME` is unset on Linux. On macOS the default is `~/Library/Application Support/ovrcr/config.toml`. An `OVRCR_CONFIG` environment variable may override the location for tests and isolated use.

OVRCR writes a temporary file in the same directory, syncs it, and atomically renames it over the previous file. A corrupt file stops startup with a useful error. OVRCR never replaces corrupt configuration with an empty registry.

Example:

```toml
[[projects]]
name = "consigint"
repo = "/Users/example/Code/consigint"
workspace_root = "/Users/example/Code/workspaces/consigint"

[[projects.workspaces]]
name = "worktree-lifecycle"
path = "/Users/example/Code/workspaces/consigint/worktree-lifecycle"
branch = "feature/worktree-lifecycle"
```

Project names are globally unique. Workspace names are unique within a project. OVRCR ignores worktrees created outside OVRCR and never adds them during a scan.

Sessions are not written to this file.

## Project commands

```text
ovrcr project add NAME REPOSITORY --workspace-root DIRECTORY
ovrcr project list
ovrcr project remove NAME
```

Adding a project canonicalizes and validates the repository and workspace-root paths. Removing a registration refuses to proceed while it contains any recorded workspace. Removing a registration never removes a repository, worktree, or branch.

## Workspace lifecycle

Create a worktree with a new branch:

```bash
ovrcr workspace create \
  --project consigint \
  --name worktree-lifecycle \
  --new-branch feature/worktree-lifecycle \
  --base main
```

Create a worktree from an existing branch:

```bash
ovrcr workspace create \
  --project consigint \
  --name worktree-lifecycle \
  --branch feature/worktree-lifecycle
```

`--new-branch` and `--branch` are mutually exclusive. `--base` is required with `--new-branch` and rejected with `--branch`. OVRCR never guesses whether a missing branch name is a typo.

The workspace name determines its directory beneath the registered workspace root. Creation rejects path traversal, an existing destination, an invalid ref, and a branch checked out elsewhere. OVRCR invokes `git` directly with fixed arguments and without a shell. It clears inherited Git repository and object-selection environment variables before each Git subprocess.

After Git creates the worktree, OVRCR records it and launches `$SHELL` at the worktree root as the workspace's `local` session. If shell creation fails, OVRCR keeps the valid worktree and record, reports the partial result, and permits a later session retry.

Remove a worktree with:

```bash
ovrcr workspace remove --project consigint --name worktree-lifecycle
```

Removal refuses to proceed until all live sessions are killed and all retained exited-session records are explicitly removed. It also refuses tracked changes, untracked files, an unexpected canonical path, or disagreement between the registry and `git worktree list --porcelain`.

Successful removal uses ordinary `git worktree remove`. It removes the workspace record and preserves the Git branch. The MVP has no force-removal option.

## Session lifecycle

Create an agent session with:

```bash
ovrcr new \
  --project consigint \
  --workspace worktree-lifecycle \
  --name "review cleanup" \
  -- codex
```

The command always starts at the workspace root. The MVP does not accept an alternate working directory. If no command follows `--`, OVRCR starts `$SHELL`.

Session names are unique within a workspace. The server assigns a short ID unique for its current lifetime. The sidebar label defaults to the basename of the launched executable; `--label TEXT` overrides it.

The server creates each session in its own process group. It initializes the PTY using the connected dashboard's terminal-pane dimensions, or 120 columns by 40 rows when detached.

Each session owns:

- Its ID, name, label, command, PID, and start time.
- Its project and workspace identifiers.
- A PTY master, child handle, and synchronized writer.
- One blocking PTY reader thread.
- One child-waiter thread.
- A synchronized `vt100` parser containing the current screen.

After the child exits, the PTY reader drains through EOF and the process group disappears. The server processes all queued output before marking the session exited and removable. Until then it remains non-removable, including when descendants outlive the child. The server retains the exited session and its final screen. Remove it with:

```text
ovrcr session remove ID
```

Removal rejects a live session. Stop a live session with:

```text
ovrcr kill ID
```

The server sends `SIGTERM` to the process group, waits up to five seconds, and sends `SIGKILL` if any member remains. The child waiter, PTY drainage, and final parser updates must complete before the session becomes removable.

## Terminal data flow

Every PTY reader drains output, including output from background sessions. Readers send chunks through bounded, lossless queues to one ordered dispatcher, which feeds each session's `vt100` parser. Queue saturation applies backpressure while parsing catches up; raw terminal bytes are never discarded before parsing. Dashboard delivery is separate and nonblocking for the dispatcher, so a slow or detached dashboard cannot stop PTY drainage.

The child waiter sends its exit event only after the reader has queued every final byte and the process group has disappeared. The dispatcher applies those bytes before the exit event. Session removal and shutdown wait for this completed state.

Only the selected session streams incremental output to the dashboard. When selection changes or the dashboard reattaches, the server first sends a formatted current-screen snapshot. The dashboard resets its local parser from that snapshot and then processes incremental chunks.

The dashboard sends input only for the selected session while in terminal mode. It converts Crossterm key and paste events into terminal byte sequences.

The dashboard calculates the rows and columns available inside the right terminal pane. Selection and outer-terminal resize events update the selected PTY and parser, then notify the child process group with `SIGWINCH`. Background PTYs retain their last dimensions until selected.

The MVP retains the current terminal screen only. It has no OVRCR copy mode or historical scrollback.

## Dashboard

The dashboard follows the approved mockup:

- A title bar.
- A fixed-width project, workspace, and session tree on the left.
- PID and elapsed time above the selected terminal.
- One selected terminal on the right.
- A key-hint footer.
- Catppuccin-style colors and Nerd Font icons, with readable text fallbacks.

Projects and workspaces sort alphabetically. Each workspace shows its `local` shell first, followed by other sessions in creation order. Session rows show the session name, command label, elapsed time, and `ctx -`.

OVRCR does not infer active, quiet, waiting, blocked, or context-usage states. Exited sessions appear dimmed; their selected metadata shows `pid closed`.

### Input modes

Browse mode supports:

- `j`, `k`, and arrow keys to select visible sessions.
- Enter to enter terminal mode.
- `q` to detach the dashboard.
- Mouse clicks on session rows and project or workspace headers.
- Collapsing and expanding project and workspace nodes.

Terminal mode sends keyboard and paste input to the selected PTY. `Ctrl-g` returns to browse mode. The footer always identifies the current mode and escape chord.

Mouse interaction is limited to the sidebar in browse mode. The MVP does not forward terminal mouse protocols and does not resize the sidebar.

## Commands

```text
ovrcr                                      open the dashboard
ovrcr project add|list|remove ...          manage registered repositories
ovrcr workspace create|remove ...          manage OVRCR Git worktrees
ovrcr new [options] -- COMMAND              create a session
ovrcr list                                 list sessions
ovrcr kill ID                              stop one process group
ovrcr session remove ID                    discard one exited session
ovrcr shutdown                             stop an empty server
ovrcr shutdown --kill                      stop all sessions and the server
```

Every mutating command returns a nonzero exit status and a concrete message on refusal or partial failure.

## Local protocol

The Unix socket protocol uses length-prefixed typed frames. Requests and events include:

- Dashboard registration and initial hierarchy.
- Project and workspace operations.
- Session create, list, kill, and remove.
- Session selection and current-screen snapshot.
- PTY input and output bytes.
- PTY resize.
- Hierarchy and metadata changes.
- Structured error responses.

The decoder enforces a fixed maximum frame length and rejects unknown or malformed messages. Protocol compatibility between different OVRCR binary versions is outside the MVP; the server and clients are expected to use the same executable version.

If a dashboard falls behind, the server discards its queued incremental output and schedules one resynchronization notification. The socket writer delivers that notification when it can make progress, even after PTY output stops. The dashboard requests a fresh current-screen snapshot; the server queues that snapshot before resuming incremental output. Already transmitted bytes may arrive before the notification. Responses and lifecycle events are preserved; if the bounded queue cannot accept them, the server disconnects the dashboard so it can reattach. Socket writes never block the parser dispatcher.

## Failure handling

- **Dashboard disconnect:** Unsubscribe it and keep all sessions running.
- **Control client disconnect:** Finish any operation the server already accepted and retain its result.
- **PTY EOF or child exit:** Complete both PTY drainage and child waiting, confirm process-group disappearance, then apply all final output before publishing the exited state and retaining the final screen.
- **Server startup race:** The successful socket owner wins; other processes reconnect.
- **Stale socket:** Remove it only after a connection attempt proves that no server answers.
- **Malformed protocol input:** Close that client without affecting sessions.
- **Git validation failure:** Make no config or filesystem change.
- **Shell failure after worktree creation:** Keep the valid worktree and registry entry.
- **Registry write failure after Git creation:** Keep the worktree, report the exact partial failure, and do not claim that OVRCR recorded it.
- **TUI exit or panic:** Restore raw mode, cursor visibility, mouse capture, and the outer terminal's alternate screen.
- **Shutdown with sessions:** Refuse unless `--kill` is explicit.

## Verification

### Unit tests

- CLI command and mutual-exclusion rules.
- Project, workspace, and session name uniqueness.
- Workspace path containment.
- Registry parsing and atomic replacement.
- Stable tree ordering.
- Key encoding and browse/terminal mode transitions.
- Protocol frame-size and malformed-message handling.
- Ratatui layout snapshots using its in-memory test backend.

### Integration tests

- Register projects backed by real temporary Git repositories.
- Create worktrees for new and existing branches.
- Refuse branches checked out elsewhere.
- Refuse removal with tracked changes or untracked files.
- Refuse removal while live or retained sessions remain.
- Spawn a real shell in a PTY and exchange input and output.
- Detach the dashboard without terminating the child.
- Reattach and reconstruct the current terminal screen.
- Propagate PTY dimensions and `SIGWINCH`.
- Terminate a process group that contains descendant processes.
- Retain and explicitly remove exited sessions only after their final output is parsed.
- Recover a slow dashboard after a finite output burst stops, with bounded raw-output and dashboard queues.
- Exercise 50 lightweight concurrent shell sessions.

Tests use bounded waits and observable process, PTY, Git, and socket state. They do not use arbitrary sleeps as proof of lifecycle completion.

### Manual acceptance

Run a real supported coding-agent CLI and verify:

- Full-screen drawing, colors, and cursor placement.
- Keyboard input and bracketed paste.
- Outer-terminal resize behavior.
- Sidebar selection by keyboard and mouse.
- Browse and terminal mode transitions.
- Detach and reattach with the agent still running.
- Workspace creation, automatic local shell creation, and guarded deletion.

## Deferred work

- Database-backed project, workspace, and session records.
- A migration from the MVP config file to that database.
- Per-session keeper processes that permit server restart and live PTY reattachment.
- Agent adapters that save conversation identifiers and restore work by relaunching an agent's resume command.
- Agent-specific status and context usage.
- External worktree discovery.
- Multiple dashboards.
- Split panes.
- Pause and resume.
- Command palette and in-dashboard creation.
- OVRCR scrollback and copy mode.
- Terminal mouse forwarding.
- Sidebar resizing.

The database alone will not restore a lost PTY. Future reattachment requires a surviving PTY owner; future restoration requires relaunching or agent-specific resume support.

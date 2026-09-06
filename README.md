# OVRCR

OVRCR is a small terminal multiplexer for one user running many command line
agent sessions. It groups sessions by registered Git project and workspace,
keeps their PTYs alive while the dashboard detaches, and reconstructs the
current screen when it reconnects.

## Install

OVRCR targets macOS and Linux and requires Rust. Build the binary with:

```sh
cargo build --release
install -m 755 target/release/ovrcr ~/.local/bin/ovrcr
```

The server uses `$XDG_RUNTIME_DIR` on Linux and `$TMPDIR` on macOS for its
private Unix socket. Set `OVRCR_SOCKET` and `OVRCR_CONFIG` when running an
isolated instance or a test fixture.

## Register a project

OVRCR manages only registered repositories and workspaces it created:

```sh
ovrcr project add consigint /Users/example/Code/consigint \
  --workspace-root /Users/example/Code/workspaces/consigint
ovrcr project list
```

Create a workspace from a new branch:

```sh
ovrcr workspace create --project consigint --name cleanup \
  --new-branch feature/cleanup --base main
```

Or use an existing branch:

```sh
ovrcr workspace create --project consigint --name cleanup \
  --branch feature/cleanup
```

The first form requires `--base`; the second rejects it. Workspace creation
also starts the workspace's `local` shell session.

## Start sessions

Launch an agent at the workspace root:

```sh
ovrcr new --project consigint --workspace cleanup --name "review cleanup" \
  -- codex
```

Use `--label TEXT` to change the sidebar label. With no command after `--`,
OVRCR launches `$SHELL`.

Run `ovrcr` with no subcommand to open the dashboard. Browse mode uses `j`,
`k`, and the arrow keys to select sessions; Enter enters terminal mode; `q`
detaches. Terminal mode sends keyboard and bracketed-paste input to the
selected PTY. Press Ctrl-g to return to browse mode. Mouse clicks select and
collapse sidebar rows while browsing. The dashboard shows one selected
terminal and the current `ctx —` field. Sidebar sessions use three lines: the
session name, its label, and elapsed runtime with context usage. Context usage
remains unknown in the MVP. The selected session is highlighted across all three
lines; clicking any of those lines selects it.
Idle sessions leave their status slot blank; an explicit busy state animates a braille spinner.
Activity defaults to idle until agent hooks are connected; terminal output and
process liveness do not imply that an agent is busy.
In Browse mode, press `p` to pause the selected live session or `r` to resume
it. The server is authoritative for the phase, so a paused row is shown only
after the server's session refresh is applied. Enter on a paused row stays in Browse
mode and reports that `r` will resume it. Keyboard input, bracketed paste, and
CLI terminal sends are rejected while a session is paused; Ctrl-g remains
available in Terminal mode so you can return to Browse and resume it.
Pause records successful signal delivery to the original process group; it does
not synchronously confirm that every group member has stopped. Input admitted
before pause may finish writing or execute after resume, and output already
buffered before pause may appear after pause.
Pause/resume is a process lifecycle control separate from agent activity. It
does not declare an agent idle, cancel remote agent work, or replace the
hook-owned activity state.

Dashboard output is delivered through a bounded queue so a detached or slow
dashboard can reattach and refresh the current screen. Output backlog is
coalesced into a per-session refresh when necessary; control responses and
lifecycle events are preserved, and the dashboard disconnects if the queue
cannot accept one of those messages.

Detaching leaves the server, PTYs, and child process groups running. Run
`ovrcr` again to reattach and rebuild the selected terminal from its current
screen. Reattach works while the original dashboard is gone; the MVP does not
restore a PTY after a server crash or reboot.

## Control terminals from scripts

The `project`, `workspace`, and `terminal` groups also accept the plural names
`projects`, `workspaces`, and `terminals`. `project create` aliases `project add`;
`project delete` and `workspace delete` alias their guarded `remove` commands.

```sh
ovrcr project list
ovrcr project get consigint
ovrcr workspace list --project consigint
ovrcr workspace get --project consigint --name cleanup
ovrcr terminal list --project consigint --workspace cleanup
```

`project list` shows projects only. `ovrcr list` retains the complete hierarchy.
Terminal lists include running sessions and retained exited records. A workspace
filter requires `--project`. Names and terminal IDs are explicit; commands do
not infer a target from the current directory.

Read commands do not start a server. When the server is absent, project and
workspace queries read the persisted registry and terminal lists are empty.
Reading or controlling a missing terminal returns an error.

### Launch, send, read, and close

This example starts with an existing registered repository and creates a clean
worktree. Replace the repository and workspace-root paths with your own:

```sh
ovrcr project add example /path/to/repo --workspace-root /path/to/workspaces
ovrcr workspace create --project example --name cli-demo \
  --new-branch feature/cli-demo --base main
terminal_id=$(ovrcr terminal create --project example --workspace cli-demo \
  --name shell -- /bin/sh)
ovrcr terminal send "$terminal_id" --text "printf 'hello from CLI\\n'"
ovrcr terminal read "$terminal_id"
ovrcr terminal read "$terminal_id" --max-lines 5
ovrcr terminal close "$terminal_id"
# Workspace creation also started a terminal named local. Find and close it:
ovrcr terminal list --project example --workspace cli-demo
ovrcr terminal close LOCAL_TERMINAL_ID
ovrcr workspace remove --project example --name cli-demo
ovrcr project remove example
```

Arguments after `--` are passed directly to the executable. With no executable,
`terminal create` launches `$SHELL`. Sessions start at the workspace root.

`terminal send` respects bracketed-paste mode and then sends Enter. Add
`--no-submit` to omit that final Enter. Embedded newlines remain part of the
text, so a program without bracketed-paste support may process those newlines
as input. A successful send means the input was written; it does not establish
program readiness or completion. A read immediately after send can therefore
show the earlier screen; read again to observe subsequent output.

`terminal read` returns plain text from the current active screen, including
an exited terminal's retained final screen. `--max-lines N` selects the last N
text lines and requires a positive number. It does not expose scrollback.
CLI reads and sends do not select or resize the dashboard terminal.

`terminal close` stops the whole process group, waits for cleanup, and removes
its record. Cleanup failure leaves the record available. To retain the final
screen, use `terminal kill ID`, then `terminal remove ID` when finished.
The legacy `kill ID` and `session remove ID` commands remain available.

### JSON output

Add `--json` to resource commands for machine-readable output. The mode is
explicit and is not selected automatically in CI or an agent environment.

```sh
ovrcr project list --json
ovrcr workspace get --project consigint --name cleanup --json
ovrcr terminal list --project consigint --json
ovrcr terminal read 7 --json
```

Resource lists return arrays. `ovrcr list --json` returns the legacy hierarchy as
an array of projects with nested `workspaces`, each containing `terminals`.
Get commands and terminal creation return objects. Other
successful mutations return `{"ok":true}`. Project records include `name`,
`repo`, `workspace_root`, and `workspace_count`; workspace records include
`project`, `name`, `path`, `branch`, and `terminal_count`. Terminal records use
numeric `id`, `phase` (`running` or `exited`), and nullable `exit_code` and
`exit_signal`, alongside ownership, label, PID, and start time. Screen reads
return `id`, `rows`, `cols`, and `text`.

Runtime errors exit 1 and, with `--json`, write an
`{"error":{"code":"NotFound","message":"…"}}` object to stderr. Successful
results go to stdout. Argument errors retain normal help diagnostics and exit
2, including with `--json`.

### Agent hook reporting

Managed sessions receive `OVRCR_HOOK_SOCKET`, `OVRCR_SESSION_ID`, and
`OVRCR_HOOK_TOKEN` in their child environment. A provider hook can report an
explicit activity state with:

```sh
ovrcr report activity --state busy --sequence 1
ovrcr report activity --state waiting-input --sequence 2
ovrcr report activity --state idle --sequence 3
```

The report command requires those inherited identity variables, uses only the
inherited hook socket, and has a one-second total deadline. Successful reports
are silent, including with `--json`. The accepted states are `unknown`, `idle`,
`busy`, `waiting-input`, and `error`.

Claude Code command hooks can translate supported hook events into activity
reports. Add these entries manually to the existing `~/.claude/settings.json`;
retain unrelated settings and handlers. If `ovrcr` is not on the provider's
`PATH`, replace it with the installed absolute OVRCR path.

```json
{
  "hooks": {
    "SessionStart": [{"hooks":[{"type":"command","command":"ovrcr report claude --stdin-json","timeout":2}]}],
    "UserPromptSubmit": [{"hooks":[{"type":"command","command":"ovrcr report claude --stdin-json","timeout":2}]}],
    "PreToolUse": [{"hooks":[{"type":"command","command":"ovrcr report claude --stdin-json","timeout":2}]}],
    "PermissionRequest": [{"hooks":[{"type":"command","command":"ovrcr report claude --stdin-json","timeout":2}]}],
    "PostToolUse": [{"hooks":[{"type":"command","command":"ovrcr report claude --stdin-json","timeout":2}]}],
    "PostToolUseFailure": [{"hooks":[{"type":"command","command":"ovrcr report claude --stdin-json","timeout":2}]}],
    "Stop": [{"hooks":[{"type":"command","command":"ovrcr report claude --stdin-json","timeout":2}]}],
    "StopFailure": [{"hooks":[{"type":"command","command":"ovrcr report claude --stdin-json","timeout":2}]}],
    "SessionEnd": [{"hooks":[{"type":"command","command":"ovrcr report claude --stdin-json","timeout":2}]}]
  }
}
```

The adapter reads only the provider JSON on standard input. It maps
`SessionStart` and `Stop` to `idle`, prompt and tool events to `busy`,
`PermissionRequest` to `waiting-input`, `StopFailure` to `error`, and
`SessionEnd` to `unknown`. Events containing `agent_id`, unknown events, and
notifications are ignored. Malformed input, an unavailable server, and report
timeouts are fail-open and produce no stdout; add `--verbose` for a bounded
diagnostic on stderr.

Start each root provider process as its own managed session so its inherited
capability identifies the correct PTY:

```sh
ovrcr new --project demo --workspace hooks --name agent -- claude
```

Do not share one OVRCR PTY between independent agent roots. To uninstall,
remove only these handlers from the existing settings file and restart the
agent session. OVRCR does not install or modify provider settings.

### Upgrading a running server

The new control commands require the updated server binary. An already-running
server continues running its original version after a rebuild. Close its
terminals and run `ovrcr shutdown`, then launch the updated binary. If you
intend to stop all sessions together, the existing `ovrcr shutdown --kill`
command does that. The CLI never stops an old server automatically or restores
its lost PTYs. The project/workspace registry requires no migration.

## Session lifecycle

List sessions, stop a process group, and remove an exited record:

```sh
ovrcr list
ovrcr pause SESSION_ID
ovrcr resume SESSION_ID
ovrcr kill SESSION_ID
ovrcr session remove SESSION_ID
```

`pause ID` sends SIGSTOP to the original process group owned by that session;
`resume ID` sends SIGCONT to the same group. Both commands require a live,
managed session and leave the session record in place. While paused, the server
rejects input admission until `resume ID` succeeds.

`kill` sends SIGTERM to the whole managed process group and uses SIGKILL after
the five-second grace period when members remain. A stopped group is resumed
with SIGCONT during cleanup so its TERM handlers and waiters can run; OVRCR
then waits for the final PTY output, reader and child-waiter completion, and
group disappearance before reporting successful cleanup. A session stays in
the hierarchy after exit until `session remove` is requested, so its final
screen remains available.

Pause and termination target the original process group established for the
OVRCR-owned PTY. OVRCR cannot adopt a process launched through another terminal
or PTY, and it does not promise to freeze descendants that move into another
group or to pause remote work or services. External job control can change
membership or stopped state, and the inherited PID/PGID reuse race remains a
limit of the ownership check; this is not a process-identity sandbox.

Workspace removal is guarded. Every session must be stopped and removed, the
worktree must have a clean Git status, its canonical path must match the
registry, and Git's worktree list must agree with the registry:

```sh
ovrcr workspace remove --project consigint --name cleanup
ovrcr project remove consigint
```

Removal uses ordinary `git worktree remove` and preserves the branch. OVRCR
never removes a repository or an unregistered worktree.

## Shutdown

An empty server can be stopped with:

```sh
ovrcr shutdown
```

This refuses while sessions remain. `ovrcr shutdown --kill` terminates every
managed process group and then stops the server.

## Disposable repository transcript

The following transcript uses an isolated temporary repository, config, and
socket. The trap stops the disposable server and removes the fixture:

```sh
$ d=$(mktemp -d)
$ export OVRCR_CONFIG="$d/config.toml"
$ export OVRCR_SOCKET="$d/server.sock"
$ trap 'if [ -e "$OVRCR_SOCKET" ]; then ovrcr shutdown --kill >/dev/null 2>&1 & p=$!; i=0; while kill -0 "$p" 2>/dev/null && [ "$i" -lt 150 ]; do sleep 0.1; i=$((i + 1)); done; status=0; if kill -0 "$p" 2>/dev/null; then kill -KILL "$p" 2>/dev/null || true; wait "$p" 2>/dev/null || true; status=124; else wait "$p"; status=$?; fi; if [ "$status" -ne 0 ] || [ -e "$OVRCR_SOCKET" ]; then echo "cleanup failed; preserving $d" >&2; exit 1; fi; fi; rm -rf "$d"' EXIT
$ git -C "$d" init -b main
$ git -C "$d" config user.name OVRCR
$ git -C "$d" config user.email ovrcr@example.invalid
$ printf 'fixture\n' > "$d/README"
$ git -C "$d" add README && git -C "$d" commit -m initial
$ mkdir -p "$d/workspaces"
$ ovrcr project add fixture "$d" --workspace-root "$d/workspaces"
$ ovrcr workspace create --project fixture --name demo \
    --new-branch feature/demo --base main
$ agent_id=$(ovrcr new --project fixture --workspace demo --name agent -- sh)
$ local_id=$(ovrcr list | awk '$1 == "session" && $3 == "local" { print $2; exit }')
$ printf 'created agent session %s and local session %s\n' "$agent_id" "$local_id"
$ ovrcr list
project fixture
  workspace demo
    session $local_id local
    session $agent_id agent
$ ovrcr
# select agent, press Enter, then Ctrl-g and q
$ ovrcr
# the agent's current screen is still present; press q
$ ovrcr kill "$agent_id"
$ ovrcr session remove "$agent_id"
$ ovrcr kill "$local_id"
$ ovrcr session remove "$local_id"
$ ovrcr workspace remove --project fixture --name demo
$ ovrcr project remove fixture
$ ovrcr shutdown
$ test ! -e "$OVRCR_SOCKET"
```

The headless PTY wrapper exercises rendered terminal state, input and paste,
resize, mouse selection, detach, reattach, and bounded cleanup:

```sh
rtk cargo test --test terminal_acceptance -- --nocapture
```

## Test in a GUI window (macOS)

Agents: follow the [computer-use testing guide](docs/testing-computer-use.md) for the smoke check and cleanup evidence.

Run the optional development helper with Rust 1.95 or newer, `just`, and `rtk`:

```sh
rtk proxy just gui
```

This builds `target/OVRCR GUI.app` and opens the real dashboard inside a
terminal window. Each launch creates temporary repositories, configuration,
socket, and server with two projects, four workspaces, and ten shell sessions.
The agent/model labels and example output are demo fixtures; all sessions run
local shells. It inherits your shell environment while overriding the demo paths
and terminal capabilities. The helper uses an installed JetBrains Mono Nerd Font
Mono when available, with a monospace fallback.

Click the terminal to focus it. Select a session and press Enter, then type
commands or paste with Cmd-V. Ctrl-g returns to browse mode,
where sidebar clicks select sessions and `q` detaches. Click **Restart dashboard**
to reattach to the same demo. Resizing the window resizes the selected session.

Computer-use tools can address the app as `dev.ovrcr.gui` or by its bundle path.
The accessibility tree exposes the terminal and its individual screen rows.
Closing the window stops the demo server and sessions and removes the temporary
fixture. A cleanup failure reports the retained fixture path. Changes made in
the demo are disposable; the helper does not open your normal ovrcr instance.

The GUI dependency is enabled only by the `gui` feature. Ordinary builds and
`cargo run` still use the CLI. To run the helper's focused tests:

```sh
rtk proxy cargo test --features gui --test gui
```

## Feature roadmap

Future additions, with priorities and release dates still to be decided:

- [ ] Split panes to view multiple sessions side by side.
- [ ] Historical scrollback to revisit output beyond the current screen.
- [ ] Copy mode to select and copy terminal output with the keyboard.
- [x] Pause and resume controls for sessions.
- [x] Agent hooks to report agent-specific activity and status.
- [ ] Context usage accounting for agent sessions.
- [ ] Mouse forwarding to applications running inside a terminal.
- [ ] Multiple dashboards connected to the same server.
- [ ] Session restore after a server crash or reboot, including saved session
  metadata, new PTYs, and agent conversation resumption where supported.

The current release supports one dashboard at the tested 50-session scale
using blocking I/O and threads. It retains each session's current terminal
screen and keeps live PTYs and session metadata in memory. Reattach connects
to a surviving PTY; a server crash or reboot loses those live sessions.

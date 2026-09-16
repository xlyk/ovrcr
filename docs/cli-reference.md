# CLI reference

Full behaviour of the `ovrcr` command line. The
[README](../README.md#the-cli) covers the common commands; this page is the
contract scripts can rely on. Every command accepts `--help`.

## Command groups

| Group | Purpose |
| --- | --- |
| `project` (`projects`) | Register and inspect Git projects |
| `workspace` (`workspaces`) | Create and remove worktree workspaces |
| `terminal` (`terminals`) | Create, read, send to, and close terminals |
| `task` (`tasks`), `run` (`runs`) | Scheduled Pi tasks; see [scheduled tasks](scheduled-tasks.md) |
| `service` | Install, start, stop, or remove the background service |
| `agent` | Supervise Claude Code, print setup, and inspect integration health |
| `report` | Report agent activity, context, or status-line metrics from a provider hook |
| `session` | Inspect or remove a session record |
| `server` | Run the server in the foreground |

`project create` aliases `project add`. `project delete` and `workspace delete`
alias their guarded `remove` commands. The legacy top-level `new`, `list`,
`kill`, `pause`, `resume`, and `session remove` commands remain available.

Resource targets and terminal IDs are explicit. No command infers a target from
the current directory. A new terminal can omit its name for automatic titles.

## Which commands start a server

`new`, `terminal create`, `terminal reopen`, `terminal acknowledge-stopped`,
`project add`, `workspace create`, and the dashboard start a server on demand.

Read commands never start one. They never migrate the registry. With no server
running, project and workspace queries read `config.toml.sqlite3` after migration.
They read the preserved `config.toml` if the database is absent or still empty and
uninitialized after an interrupted import. A foreign, unreadable, or incompatible
database is an error, not a TOML fallback. Terminal lists include retained rows.
Reading or controlling a missing terminal returns an error.

Removal and kill commands (`project remove`, `workspace remove`,
`terminal kill`, `terminal close`, `terminal remove`, `kill`, `session remove`)
never start a server. With none running they fail with `OVRCR server is not
running`: only the server mutates retained records or controls processes.

## Request timeouts

Every request is bounded. An ordinary request fails after 30 seconds without a
response; `kill`, `close`, and `shutdown` after 60 seconds.

## Inspect resources

```sh
ovrcr project list
ovrcr project get consigint
ovrcr workspace list --project consigint
ovrcr workspace get --project consigint --name cleanup
ovrcr terminal list --project consigint --workspace cleanup
```

`project list` shows projects only; `ovrcr list` prints the complete hierarchy as
`project`, `  workspace`, and `    session <id> <display-name> <phase>` lines. Terminal
lists include running sessions and retained exited records. A `--workspace` filter
requires `--project`.

## Launch, send, read, and close

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
`terminal create` and `new` launch `$SHELL`. Sessions start at the workspace root.
`project add` creates the workspace root if it is missing.
`--label TEXT` sets launch metadata such as the executable or agent label.
Omit `--name` for a stable workspace-based fallback name and an automatic display
title that follows the running app. Supply `--name TEXT` to start with a pinned
title instead.

```sh
ovrcr terminal rename ID "Review login"  # pin a display title
ovrcr terminal rename ID --automatic    # follow app titles again
ovrcr terminal reopen ID                # reopen a retained row in a fresh shell
ovrcr terminal reopen ID --ack-stopped  # confirm previous processes stopped, then reopen
ovrcr terminal acknowledge-stopped ID   # resolve ownership without launching
```

Rename changes the display title, not the stable session name or ID. Reopen
starts a fresh shell in the same session row. It does not keep the previous
process output. `--ack-stopped` is required when the inventory says previous
agent or background processes may still be running; Retry is not that
confirmation. `acknowledge-stopped` records that confirmation without launching
a session, but starts the server on demand when it is not running. Agent rows
cannot be resumed. Reopen starts the server if it is not running. With `--json`,
reopen returns the terminal object and rename returns `{"ok":true}`.

A natural exit does not prove that background jobs stopped. Such a row keeps
`recovery.requires_ack=true`, even while its current phase is `exited`. Confirm
that the old processes are gone with `terminal reopen ID --ack-stopped`, or use
`terminal acknowledge-stopped ID` before closing or removing the row. A verified
different boot also resolves prior-run ownership; no process is signalled by a
saved PID. A successfully verified controlled stop does not need another
acknowledgement; a raced, failed or unverifiable stop does not certify cleanup.

`terminal send` respects bracketed-paste mode and then sends Enter; `--no-submit`
omits that final Enter. Embedded newlines remain part of the text, so a program
without bracketed-paste support may process them as input. A successful send means
the input was written: it does not establish program readiness or completion. A
read immediately after a send can therefore show the earlier screen; read again to
observe subsequent output.

`terminal read` returns plain text from the current active screen, including an
exited terminal's retained final screen. `--max-lines N` selects the last N text
lines and requires a positive number. It does not expose scrollback. CLI reads and
sends do not select or resize the dashboard terminal.

`terminal close` stops the whole process group, waits for cleanup, and removes the
record. Cleanup failure leaves the record available. To retain the final screen,
use `terminal kill ID`, then `terminal remove ID` when finished.

## Agent reporting commands

| Command | Contract |
| --- | --- |
| `agent run --provider claude -- claude [ARGS...]` | Supervise an exact Claude Code 2.1.267 or 2.1.268 fresh interactive invocation, or a separate-token `--resume UUID` invocation, inside an OVRCR PTY. Exact 2.1.268 also accepts separate-token `-r UUID`. Does not start a server. |
| `agent run pi -- pi [ARGS...]` | Supervise an interactive Pi launch inside an OVRCR PTY; the owned reporting extension is loaded beside the user's own. Reports Busy, Ready (confirmed at Pi's settled boundary), Error and Idle; creates Unread and Ready alerts. Help, version, print, RPC, JSON, export and package commands run native with reporting unavailable. |
| `agent run omp -- omp [ARGS...]` | Supervise an interactive Oh My Pi launch inside an OVRCR PTY; the owned reporting extension is loaded beside the user's own. Reports Busy, Ready (observed at an end without continuation), Error and Idle; creates Unread and Ready alerts. Headless modes run native with reporting unavailable. |
| `agent setup pi\|omp --print` | Print the managed-launch contract; nothing to write. |
| `agent doctor pi\|omp --json [--session ID] [--executable PATH]` | Probe `--version` (tested / unverified / unknown), inspect an optional `--session` (binding, delivery, activity, health, unread) and print remediation; no server is started. |
| `agent setup codex --print [--settings PATH]` | Print composed Codex 0.153.0 TOML with five synchronous direct-exec reporters; preserve existing values and handler order. Explicit installation and native trust review are required. See [Codex setup](codex-reporting-setup.md). |
| `agent doctor codex --json [--settings PATH] [--executable PATH]` | Defaults to `codex`; probes only `--version` and checks supplied TOML without a server or provider conversation. Hook trust/delivery and release acceptance remain unverified. |
| `agent setup claude --print [--settings PATH]` | Print composed JSON; migration and removal notes go to stderr. Does not write provider settings. |
| `agent doctor claude --json [--settings PATH] [--session ID] [--executable PATH]` | Probe the executable, report the detected version and exact supported-version list, and inspect supplied configuration and optional session health. Without `--session`, uses inherited `OVRCR_SESSION_ID` when present. Does not start a server. |
| `session usage ID` | Emit JSON containing `agent_epoch`, raw `agent`, `reporting_unavailable`, and separate `measurement_age_ms` fields. Does not start a server. |
| `session context ID` | Emit the legacy context sample and stale flag as JSON. Does not start a server. |
| `report claude --stdin-json` | Route typed Claude command hooks through inherited reporting credentials. |
| `report claude-statusline --stdin-json [--render-command COMMAND]` | Attempt managed context/cost reporting while rendering the status line. An external renderer receives the original input bytes. |

Terminal inventory includes the same agent snapshot and independent measurement
ages as `session usage`. Each component age counts from the last sample whose
value changed, not from the last delivery, and `context_stale` reports the same
five-minute rule over the context component's age. Unknown observations and
unavailable ages are JSON `null`; zero usage or cost is a known value. The `agent` snapshot contains distinct
binding, activity, metrics, and reporter health. Token and cost scopes must be
interpreted independently. No provider capability or private launcher lease is
included.

See [Claude Code setup](claude-code-setup.md) for configuration steps and
[agent reporting](agent-reporting.md) for managed and legacy semantics.

## Session lifecycle

```sh
ovrcr list
ovrcr pause SESSION_ID
ovrcr resume SESSION_ID
ovrcr kill SESSION_ID
ovrcr session remove SESSION_ID
```

`pause ID` sends SIGSTOP to the process group owned by that session and to any
job-control subgroup attached to its terminal; `resume ID` sends SIGCONT to the
same groups. Both require a live, managed session and leave the session record in
place. While paused, the server rejects input admission until `resume ID`
succeeds.

`kill` signals every process group attached to the session's terminal: the managed
process group and any job-control subgroup an interactive shell started. It sends
SIGTERM and SIGCONT first so handlers can run, sends SIGHUP to any group still
present half a second later, and uses SIGKILL after the five-second grace period
when members remain. SIGHUP is what a closed terminal window delivers and the only
signal interactive shells honour, so `local` shells exit within about half a second
instead of waiting out the grace period; a program that needs longer than that for
its SIGTERM handling must also handle SIGHUP. Processes that detach from the
terminal with `setsid` are outside the session and are not signalled. A stopped
group is resumed with SIGCONT during cleanup so its handlers and waiters can run;
OVRCR then waits for the final PTY output, reader and child-waiter completion, and
group disappearance before reporting successful cleanup. A session stays in the
hierarchy after exit until `session remove` is requested, so its final screen
remains available.

Pause and termination target the original process group established for the
OVRCR-owned PTY. OVRCR cannot adopt a process launched through another terminal or
PTY, and it does not promise to freeze descendants that move into another group or
to pause remote work or services. External job control can change membership or
stopped state, and the inherited PID/PGID reuse race remains a limit of the
ownership check. This is not a process-identity sandbox.

## Removal guards

```sh
ovrcr workspace remove --project consigint --name cleanup
ovrcr project remove consigint
```

Workspace removal requires every session stopped and removed, a clean Git status
in the worktree, a canonical path matching the registry, and agreement between
Git's worktree list and the registry. Removal uses ordinary
`git worktree remove` and preserves the branch. OVRCR never removes a repository
or an unregistered worktree. If the worktree directory was deleted outside OVRCR,
removal instead runs `git worktree prune`, but only after Git itself lists the
registered path and branch as prunable, and then drops the registry record.

## Shutdown

```sh
ovrcr shutdown
ovrcr shutdown --kill
```

`shutdown` refuses while live owned processes remain. Retained exited or stopped
rows do not block it. `--kill` terminates every live managed process group and
then stops the server.

## JSON output

Add `--json` to a resource command for machine-readable output. The mode is
explicit and is never selected automatically in CI or an agent environment.

```sh
ovrcr project list --json
ovrcr workspace get --project consigint --name cleanup --json
ovrcr terminal list --project consigint --json
ovrcr terminal read 7 --json
```

Resource lists return arrays. Get commands, terminal creation, and reopen return objects.
Other successful mutations return `{"ok":true}`. `ovrcr list --json` returns the
legacy hierarchy as an array of projects with nested `workspaces`, each containing
`terminals`.

| Record | Fields |
| --- | --- |
| Project | `name`, `repo`, `workspace_root`, `workspace_count` |
| Workspace | `project`, `name`, `path`, `branch`, `terminal_count` |
| Terminal | `id`, `run`, `kind`, `project`, `workspace`, `name`, `title`, `display_name`, `label`, `pid`, `started_unix_ms`, `phase`, `activity`, `exit_code`, `exit_signal`, `recovery`, `context_usage`, `context_stale` |
| Screen read | `id`, `rows`, `cols`, `text` |

`name` is the stable session name. `title` is its nullable effective title;
`display_name` is the title or fallback name. These titles do not imply agent
activity or reporting support.

`run` is the process-run identity of the row. `kind` is `terminal` or
`{"agent":"<preset>"}`. The agent name identifies a selected preset or detected
executable; it is not proof of resume support. `phase` is `running`, `paused`,
`exited`, `stopped`, or `interrupted`; `exit_code` and `exit_signal` are nullable
and set only for `exited`. Dormant rows have a null `started_unix_ms` rather than
inventing elapsed time from epoch. `recovery` describes ownership and resume state.
`activity` is one of `unknown`, `idle`, `busy`,
`waiting_input`, `response_ready`, or `error`, and is the latest accepted provider
observation — `waiting_input` while the terminal's `agent.input_requests` set is non-empty,
whatever activity is recorded underneath (see
[input requests](dashboard.md#input-requests)).
Capability and hook transport fields are never included. The legacy
`ovrcr list --json` hierarchy and the default human-readable list omit
`context_usage` and `context_stale` and otherwise keep their existing shape.

Runtime errors exit 1 and, with `--json`, write
`{"error":{"code":"NotFound","message":"…"}}` to stderr; successful results go to
stdout. Argument errors keep normal help diagnostics and exit 2, including with
`--json`.

## Worked example in a disposable repository

This transcript uses an isolated temporary repository, config, and socket. The
trap stops the disposable server and removes the fixture.

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
$ ovrcr list
project fixture
  workspace demo
    session $local_id local running
    session $agent_id agent running
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

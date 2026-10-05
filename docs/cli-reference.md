# CLI reference

Full behaviour of the `ovrcr` command line. The
[README](../README.md#the-cli) covers the common commands; this page is the
contract scripts can rely on. Every command accepts `--help`.

## Command groups

| Group | Purpose |
| --- | --- |
| `project` (`projects`) | Register and inspect Git projects |
| `workspace` (`workspaces`) | Create and remove worktree workspaces |
| `terminal` (`terminals`) | Create, read, send text or keys to, and close terminals |
| `task` (`tasks`), `run` (`runs`) | Scheduled Pi tasks; see [scheduled tasks](scheduled-tasks.md) |
| `service` | Install, start, stop, or remove the background service |
| `agent` | Supervise Claude Code, print setup, and inspect integration health |
| `report` | Report agent activity, context, or status-line metrics from a provider hook |
| `session` | Inspect or remove a session record |
| `server` | Run the server in the foreground |
| `settings` | Show the settings document, effective values and findings; `set` and `reset` change one setting |
| `events` | Print the running Server's event ring. Does not start a Server and does not read `events.jsonl` |

`project create` aliases `project add`. `project delete` and `workspace delete`
alias their guarded `remove` commands. The legacy top-level `new`, `list`,
`kill`, `pause`, `resume`, and `session remove` commands remain available.

Resource targets and terminal IDs are explicit. No command infers a target from
the current directory. A new terminal can omit its name for a stable generated name.

## Which commands start a server

`new`, `terminal create`, `terminal reopen`, `terminal acknowledge-stopped`,
`project add`, `workspace create`, and the dashboard start a server on demand.

Read commands never start one. They never write, migrate the registry, or launch
a shell. With no server running, project and workspace queries read the existing
`registry.sqlite3` without changing it. They read the preserved `config.toml`
if the database is absent or still empty and uninitialized after an interrupted
import. A foreign, unreadable, or incompatible database is an error, not a TOML
fallback. Terminal lists include retained rows. Reading or controlling a missing
terminal returns an error.

Removal and kill commands (`project remove`, `workspace remove`,
`terminal kill`, `terminal close`, `terminal remove`, `kill`, `session remove`)
never start a server. With none running they fail with `OVRCR server is not
running`: only the server mutates retained records or controls processes.
`workspace remove --force` acknowledges ownership-uncertain stopped sessions and
discards uncommitted changes; live sessions and active task runs still block it.
`ovrcr events` also never starts a Server. With none running it fails with
`OVRCR server is not running`. It asks the Server for the in-memory ring and
does not read `events.jsonl`.

## Request timeouts

Every request is bounded. An ordinary request fails after 30 seconds without a
response; `kill`, `close`, and `shutdown` after 60 seconds.

## Inspect resources

```sh
ovrcr project list
ovrcr project get consigint
ovrcr workspace list --project consigint
ovrcr workspace get --project consigint --branch feature/cleanup
ovrcr terminal list --project consigint --workspace feature/cleanup
```

`project list` shows projects only; `ovrcr list` prints the complete hierarchy as
`project`, `  workspace`, and `    session <id> <display-name> <phase>` lines.
Workspace lines use the current branch name. Terminal
lists include running sessions and retained exited records. A `--workspace` filter
requires `--project` and matches that current branch. Terminal JSON includes `cwd`, the original working directory;
the text table includes it as the last column, including for archived records.

`workspace get` and `workspace remove` take `--project` with `--branch` (the
current checkout branch; slashes such as `feature/cleanup` are valid) or `--path`
(the worktree). `new`, `terminal create`, and `terminal list` use `--workspace`
for that same current branch, or `--path`. A detached checkout is shown as
`detached @` followed by a short hash and is not a branch target: use `--path`.
Two workspaces on the same branch is an error that lists candidate paths; pass
`--path` to choose one. An old branch name does not match after the checkout
moves.

## Settings

```sh
ovrcr settings
ovrcr settings --json
```

`settings` with no subcommand reads the settings document locally, as the Server does at startup:
`dashboard.toml` in the instance directory, or the file `OVRCR_DASHBOARD_CONFIG` names.
It never contacts or starts a server. It prints the resolved document path, then
one line per setting with its key, owner (`Server` or `Dashboard`), source
(`default` or `document`) and effective value (`unset` when there is none). An
off consent setting (`desktop_notifications`, `title_model`, `quota.claude.probe`, and `quota.enabled` when it is false) is
followed by what to set to turn it on. `quota.enabled` defaults to true: Codex and Grok collection runs while a Dashboard is attached unless the document sets it false. Findings come last: wrong types, unknown
keys, unknown spellings, an unparseable document, and a `[quota]` table left in
the instance identity file, each with its key and line when known. The command exits 0 even
with findings.

`--json` prints one object: `path`, `read_unix_ms`, `settings` (every effective
value, typed), `rows` (`key`, `owner`, `value`, `source`, `off_state`) and
`findings` (`key`, null for the whole document; `message`; `line`, nullable).

```sh
ovrcr settings set PATH VALUE
ovrcr settings reset PATH
```

`set` changes one setting and `reset` removes it from the document so it returns
to its default. PATH is a setting path: a key such as `ready_sound` or
`quota.codex.command`, a list element such as `picker_roots[2]` or
`agents[1].argv`, or a launch choice such as `launch_choices.myproj` or
`launch_choices.myproj.kind`; quote a project name that holds a dot,
`launch_choices."my.project"`. Index len of a list appends. Quote a path or value
with brackets, quotes or spaces for the shell, for example
`ovrcr settings set 'picker_roots[0]' '~/Code'`. VALUE is TOML value
text: `true`, `"kh/"`, `["claude", "--verbose"]`, or an inline table such as
`{ name = "claude", argv = ["claude"] }` for an `agents` element and
`{ kind = "Agent", preset = "claude" }` for a launch choice. A bare word that is
not TOML is taken as is only for a string or path setting (`branch_prefix`,
`title_model`, `automatic_local_terminals`, `picker_roots[N]`, `quota.*.command`
and `home`, a launch choice's `kind` and `preset`), so
`ovrcr settings set title_model pi/gpt-5-mini` needs no quotes.

When a Server is reachable the edit goes through it: the Server checks the
value, saves, re-reads and sends the attached Dashboard its new reading, and the
command prints `Set PATH through the Server`. With no Server running the command
edits the resolved document itself with the same writer, starts no Server, and
prints `Set PATH in /path/to/dashboard.toml (no Server running)`; a Server
started later loads that value. Either way the edit keeps comments, unrelated
keys and formatting. A path that names no setting, a value of the wrong type
(the message names the expected type, such as `expected a boolean`), a missing
list element, and a document that is not valid TOML are rejected with a
non-zero exit and the document unchanged. `--json` prints `ok`, `path`, and
`document` (null when the Server saved it).

## Events

```sh
ovrcr events
ovrcr events --json
ovrcr events --follow
```

`events` prints the running Server's event ring, oldest first and newest last.
Each line is the time in UTC, the component (`titles`, `settings`, or `quota`),
the session id or provider name when the line is about one, and the one-line
message. `--json` prints that same list as one JSON array (`time_unix_ms`,
`component`, `subject`, `message`) which parses back to the ring. `--follow`
prints the ring and then each event recorded afterwards until the Server
closes the connection; with `--json`, events after the array are one JSON
object per line.

The command needs a running Server. It does not start one, and it does not
read `events.jsonl`. That file, in the instance directory, is for people and other
tools. An event message never includes a prompt, a transcript, a credential,
an account identifier, or a native body.

The Dashboard Events popup is not part of this command.

## Workspace removal

`workspace remove` is explicit confirmation to remove the clean worktree and
archive its stopped session records. Running, paused, and ownership-uncertain
sessions block removal, including uncertain archived records. Dirty-worktree,
root-repository and Git ownership protections still apply. Branches are kept.
A recorded provider-history path inside the worktree also blocks removal, even
if Git ignores that file. Preserve it outside the worktree and have the provider
report its new reference before retrying.

Each refusal names the blocking session ids, marking archived rows, which
`terminal list` hides by default. Clear an unacknowledged stopped row with
`terminal acknowledge-stopped ID` or `terminal remove ID`, or pass `--force`.
A history holder has no `--force` path: move the history and update its
reference, or `terminal remove ID` to drop the record; the worktree removal
then deletes the files with the worktree.

Registry removal and archive transitions commit together. A storage error before
Git removal or a Git refusal leaves the records unarchived. If Git removes the
worktree but the database commit fails, the command reports partial failure and
keeps the original workspace and session records visible for recovery; it does
not claim Git was rolled back. Inspect the reported path and Git worktree list
before restoring the worktree or repairing registration. Reopening refuses a
missing directory and never substitutes another repository.

The repository-root workspace cannot be removed. `project remove` still requires
removing every other workspace first. It then drops the root registration after
live and ownership-uncertain sessions are stopped or acknowledged. Repository
files stay on disk. It never cascades into retained session records. Unarchiving a row
whose workspace or project was removed keeps it visible under its original
context without registering a workspace or launching a process. Provider history
files are never deleted by record management.

## Launch, send, read, and close

```sh
ovrcr project add example /path/to/repo --workspace-root /path/to/workspaces
ovrcr workspace create --project example \
  --new-branch feature/cli-demo --base main
terminal_id=$(ovrcr terminal create --project example --workspace feature/cli-demo \
  --name shell -- /bin/sh)
ovrcr terminal send "$terminal_id" --text "printf 'hello from CLI\\n'"
ovrcr terminal keystroke "$terminal_id" :ctrl-c:
ovrcr terminal read "$terminal_id"
ovrcr terminal read "$terminal_id" --max-lines 5
ovrcr terminal close "$terminal_id"
# With automatic_local_terminals=on, workspace creation also starts a terminal
# named local. With the default (default_branch_only) it does not; create one
# explicitly with terminal create / new when needed.
# Removal keeps stopped sessions in the archive with their original paths:
ovrcr workspace remove --project example --branch feature/cli-demo
# Stop any root-workspace shell started by project add (default policy does),
# then unregister:
ovrcr terminal list --project example --workspace main
ovrcr terminal close ROOT_TERMINAL_ID   # skip when no shell was created
ovrcr project remove example
```

Arguments after `--` are passed directly to the executable. With no executable,
`terminal create` and `new` launch `$SHELL`. Sessions start at the workspace checkout.
`project add` creates the `--workspace-root` directory if it is missing and registers
the protected repository-root workspace on the detected default branch. Whether it
also starts a `local` shell follows `automatic_local_terminals` in `dashboard.toml`
(default **default branch only**). It does not switch Git.
`--label TEXT` sets launch metadata such as the executable or agent label.
Omit `--name` for a stable workspace-based name, or supply `--name TEXT` to
choose one. Application title updates are ignored for all sessions.

```sh
ovrcr terminal rename ID "Review login"  # set a display title
ovrcr terminal rename ID --reset        # restore the original session name
ovrcr terminal reopen ID                # reopen a retained row in a fresh shell
ovrcr terminal reopen ID --ack-stopped  # confirm previous processes stopped, then reopen
ovrcr terminal acknowledge-stopped ID   # resolve ownership without launching
```

Rename changes the display title, not the stable session name or ID. `--reset`
restores that original name; the legacy `--automatic` spelling remains an alias
for reset and no longer follows application titles. Manual titles and resets
survive server restarts. Reopen starts a fresh shell in the same session row
without changing its title. It does not keep the previous
process output. `--ack-stopped` is required when the inventory says previous
agent or background processes may still be running; Retry is not that
confirmation. `acknowledge-stopped` records that confirmation without launching
a session, but starts the server on demand when it is not running. Certified Agent
rows enter their managed resume command into that shell to resume the exact retained
conversation without a new prompt (Claude, Codex, Pi and Oh My Pi). Without a saved
conversation, these providers open their native resume picker instead. The shell
remains usable when the agent exits. Reopen starts the server if it is not running. With `--json`,
reopen returns the terminal object and rename returns `{"ok":true}`.

CLI inventory remains passive. The Dashboard's first visible display can recover
an eligible interrupted Agent after a verified reboot; it never acknowledges
uncertain ownership. Failed automatic attempts require explicit `terminal reopen`
or the Dashboard's Retry action, even after capacity or missing files are restored.

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

`terminal keystroke ID KEY` writes one named key to the session's current run
as the Dashboard would encode it, including the program's cursor-key mode. It is
never bracketed, even when the program has paste mode on; use `terminal send` to
paste text. `KEY` is one character or a lowercase key name between colons, after
optional `ctrl-`, `alt-`, `shift-` prefixes in that order: `:j:` and `:J:` differ,
and the names are `enter`, `escape`, `tab`, `backspace`, `space`, `colon`, `up`,
`down`, `left`, `right`, `home`, `end`, `pageup`, `pagedown`, `insert`, `delete`,
and `f1` through `f12`. `:ctrl-c:`, `:alt-down:` and `:ctrl-shift-enter:` are
valid. `shift-` applies only to named keys, so `:shift-j:` is an argument error,
as are `:hello:`, `:Enter:` and `:shift-ctrl-c:`; nothing is written. `:ctrl-g:`
reaches the program: the Dashboard's Ctrl-G intercept applies only to keys typed
into the Dashboard. For example, `:j:` then `:enter:` moves a menu that opens with
No highlighted to Yes and confirms it.

A Keystroke is refused with `Conflict` while the Dashboard is focused on that
session, since the person at the Dashboard owns its keyboard. It is allowed when
no Dashboard is attached or the Dashboard shows a different session; focus is
checked when the key is written. Paused and exited sessions refuse it, and a
request for a replaced run writes nothing. Success, `{"ok":true}` with `--json`,
means the bytes were written, not that the program handled the key.

`terminal read` returns plain text from the current active screen, including an
exited terminal's retained final screen. `--max-lines N` selects the last N text
lines and requires a positive number. It does not expose scrollback. CLI reads and
sends do not select or resize the dashboard terminal.

`terminal close ID` archives an exited or stopped row immediately. For live work,
this explicit command stops the currently owned processes and archives only after
stop succeeds; failure keeps an actionable active record. The Dashboard asks for
confirmation before stopping live work. Close captures the current run, and stale
requests cannot stop a replacement run.

Use `terminal list --archived` to inspect the archive, including offline.
`terminal unarchive ID` returns a stopped row without executing anything.
`terminal remove ID` deletes the record; it never deletes provider history files.
Archive, unarchive and deletion survive server restart. Archiving a naturally
exited row does not prove that background jobs stopped: ownership acknowledgement
may still be required before reopening an unarchived row. Agent suggestions alone
do not close sessions.

## Agent reporting commands

| Command | Contract |
| --- | --- |
| `agent run claude -- claude [ARGS...]` | Supervise a stable Claude Code >=2.1.267 fresh interactive invocation, or a separate-token `--resume UUID` invocation, inside an OVRCR PTY. The Dashboard agent picker uses this same managed route. Legacy `--provider claude` remains accepted. 2.1.268 and later compatible patches also accept separate-token `-r UUID`. Does not start a server. |
| `agent run codex -- codex [ARGS...]` | Supervise a stable Codex CLI >=0.153.0 fresh interactive invocation inside an OVRCR PTY. The Dashboard agent picker uses this same managed route. Fresh turns deliver Busy, Ready and Unread; verified root approvals support Input while questions remain unavailable. Does not start a server. |
| `agent run pi -- pi [ARGS...]` | Supervise an interactive Pi launch inside an OVRCR PTY; the owned reporting extension is loaded beside the user's own. Reports Busy, Ready (confirmed at Pi's settled boundary), Error and Idle; creates Unread and Ready alerts. Help, version, print, RPC, JSON, export and package commands run native with reporting unavailable. |
| `agent run omp -- omp [ARGS...]` | Supervise an interactive Oh My Pi launch inside an OVRCR PTY; the owned reporting extension is loaded beside the user's own. Reports Busy, Ready (observed at an end without continuation), Error and Idle; creates Unread and Ready alerts. Headless modes run native with reporting unavailable. |
| `agent setup pi\|omp --print` | Print the managed-launch contract; nothing to write. |
| `agent doctor pi\|omp --json [--session ID] [--executable PATH]` | Probe `--version` (tested / compatible untested / outside range / unknown), inspect an optional `--session` (binding, delivery, activity, health, unread) and print remediation; no server is started. |
| `agent setup codex --print [--settings PATH]` | Print composed Codex >=0.153.0 TOML with synchronous direct-exec reporters (including unfiltered SessionStart and PermissionRequest/PreToolUse/PostToolUse for Approval Input); preserve existing values and handler order. Explicit installation and native trust review are required. See [Codex setup](codex-reporting-setup.md). |
| `agent doctor codex --json [--settings PATH] [--session ID] [--executable PATH]` | Defaults to `codex`; invokes the provider only for `--version`, checks supplied TOML, and inspects optional session binding and health. Without `--session`, uses inherited `OVRCR_SESSION_ID` when present. Unavailable reporting uses `reporting_unavailable` with recognized diagnostic codes; private reason text stays redacted. Identity freezes explain fresh-invocation recovery; `remediation` remains a string. Does not start a server or provider conversation. Hook trust/delivery and release acceptance remain unverified. |
| `agent setup claude --print [--settings PATH]` | Print composed JSON; migration and removal notes go to stderr. Does not write provider settings. |
| `agent doctor claude --json [--settings PATH] [--session ID] [--executable PATH]` | Probe the executable, report the detected version, compatible range and separately tested versions, and inspect supplied configuration and optional session health. Unavailable reporting uses `session_status=reporting_unavailable` and exposes recognized diagnostic codes in `source_health.reason`; arbitrary/private reason text remains redacted. Identity freezes explain the unsupported foreground transition and fresh-invocation recovery. Without `--session`, uses inherited `OVRCR_SESSION_ID` when present. Does not start a server. |
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
ovrcr workspace remove --project consigint --branch feature/cleanup
ovrcr project remove consigint
```

Workspace removal requires every session stopped and removed, a clean Git status
in the worktree, a canonical path matching the registry, and agreement between
Git's worktree list and the registry. Target the current branch or the worktree
path; the repository-root workspace is rejected. Removal uses ordinary
`git worktree remove` and preserves the branch. OVRCR never removes a repository
or an unregistered worktree. If the worktree directory was deleted outside OVRCR,
removal instead prunes Git's registration, but only after Git itself lists the
registered path as prunable, and then drops the registry record. If Git has
already forgotten the path as well, removal only drops the registry record.

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
ovrcr workspace get --project consigint --branch feature/cleanup --json
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

This example uses an isolated temporary repository, config, and socket. The
trap stops the disposable server and removes the fixture. Session IDs in the
illustrated output use the variables captured above; actual output contains numbers.

```sh
$ d=$(mktemp -d)
$ export OVRCR_HOME="$d"
$ export OVRCR_SOCKET="$d/server.sock"
$ trap 'if [ -e "$OVRCR_SOCKET" ]; then ovrcr shutdown --kill >/dev/null 2>&1 & p=$!; i=0; while kill -0 "$p" 2>/dev/null && [ "$i" -lt 150 ]; do sleep 0.1; i=$((i + 1)); done; status=0; if kill -0 "$p" 2>/dev/null; then kill -KILL "$p" 2>/dev/null || true; wait "$p" 2>/dev/null || true; status=124; else wait "$p"; status=$?; fi; if [ "$status" -ne 0 ] || [ -e "$OVRCR_SOCKET" ]; then echo "cleanup failed; preserving $d" >&2; exit 1; fi; fi; rm -rf "$d"' EXIT
$ git -C "$d" init -b main
$ git -C "$d" config user.name OVRCR
$ git -C "$d" config user.email ovrcr@example.invalid
$ printf 'fixture\n' > "$d/README"
$ git -C "$d" add README && git -C "$d" commit -m initial
$ mkdir -p "$d/workspaces"
$ ovrcr project add fixture "$d" --workspace-root "$d/workspaces"
$ root_id=$(ovrcr list | awk '$1 == "session" { print $2; exit }')
$ ovrcr workspace create --project fixture \
    --new-branch feature/demo --base main
$ agent_id=$(ovrcr new --project fixture --workspace feature/demo --name agent -- sh)
$ local_id=$(ovrcr list | awk -v root="$root_id" -v agent="$agent_id" '$1 == "session" && $2 != root && $2 != agent { print $2; exit }')
$ ovrcr list
project fixture
  workspace feature/demo
    session $local_id local running
    session $agent_id agent running
  workspace main
    session $root_id local running
$ ovrcr
# select agent, press Enter, then Ctrl-g and q
$ ovrcr
# the agent's current screen is still present; press q
$ ovrcr kill "$agent_id"
$ ovrcr session remove "$agent_id"
$ ovrcr kill "$local_id"
$ ovrcr session remove "$local_id"
$ ovrcr workspace remove --project fixture --branch feature/demo
$ ovrcr kill "$root_id"
$ ovrcr session remove "$root_id"
$ ovrcr project remove fixture
$ ovrcr shutdown
$ test ! -e "$OVRCR_SOCKET"
```

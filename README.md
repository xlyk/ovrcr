# OVRCR

A terminal multiplexer for one person running many coding-agent sessions, grouped
by Git project and worktree.

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

## What it is

Running several coding agents at once means several long-lived terminals, each on
its own branch, each in a state you cannot see from the others. OVRCR gives those
terminals a shared home: you register a Git repository, create a workspace (a Git
worktree on its own branch), and start sessions inside it. A single server owns
every PTY, so the dashboard can detach and reattach without killing anything, and
the same sessions can be driven from a script.

Two things shape the design. OVRCR's unit of organisation is a registered project
and its worktree workspaces, not a bare window list, so creating a workspace
creates the branch, the worktree, and its `local` shell together. And OVRCR never
guesses what an agent is doing: activity and context usage come from provider
hooks that report explicitly, so a quiet terminal is not called idle.

It is a synchronous Rust program — blocking I/O and threads, no async runtime —
supporting one server, one attached dashboard, and a tested workload of 50
sessions.

## Features

- Sessions grouped by registered Git project and worktree workspace, created and
  removed with guards that refuse to touch a dirty or unregistered worktree.
- One or two side-by-side panes, with separate Browse, Terminal, Copy, and
  History input modes.
- PTYs and child process groups survive dashboard detach; reattaching rebuilds
  the current screen from the running session.
- Session rows survive a server restart as metadata. Reopen starts a fresh shell
  in the same row; it does not restore output, and agent resume is unavailable.
- 512 rows of retained scrollback per session, with keyboard selection and
  clipboard copy over OSC52.
- Pause and resume a session's process group with SIGSTOP and SIGCONT.
- Per-session agent activity and context occupancy, reported by provider hooks
  (Claude Code adapters included).
- Mouse forwarding to applications that request a tracking mode.
- A scriptable CLI that creates, sends to, reads, and closes terminals, with
  `--json` on every resource command.
- Scheduled Pi agent tasks that run in fresh worktrees, with a login service.

## Contents

- [Requirements](#requirements)
- [Install](#install)
- [Quickstart](#quickstart)
- [The dashboard](#the-dashboard)
- [The CLI](#the-cli)
- [Agent activity and context](#agent-activity-and-context)
- [Scheduled tasks](#scheduled-tasks)
- [Configuration and environment](#configuration-and-environment)
- [Upgrading a running server](#upgrading-a-running-server)
- [Troubleshooting](#troubleshooting)
- [How it works](#how-it-works)
- [Development](#development)
- [Contributing](#contributing)
- [Roadmap and limits](#roadmap-and-limits)
- [License](#license)

## Requirements

- macOS or Linux.
- Rust 1.95 or newer (the workspace is edition 2024; the optional GUI helper's
  `eframe` sets the 1.95 floor).
- Git, for projects and workspaces.
- Optional: [Pi](docs/scheduled-tasks.md) if you want scheduled tasks.

## Install

No published package yet — build from source:

```sh
git clone https://github.com/xlyk/ovrcr.git
cd ovrcr
cargo build -p ovrcr --release
install -m 755 target/release/ovrcr ~/.local/bin/ovrcr
```

Check the result, including the wire protocol version a client and server must
agree on:

```sh
$ ovrcr --version
ovrcr 0.1.0 (protocol 3)
```

## Quickstart

Register a repository and tell OVRCR where to put its worktrees. OVRCR manages
only registered repositories and workspaces it created, and creates the workspace
root on registration if it is missing:

```sh
ovrcr project add consigint ~/Code/consigint \
  --workspace-root ~/Code/workspaces/consigint
```

Create a workspace. The first form cuts a new branch and requires `--base`; the
second reuses an existing branch and rejects `--base`. Either way OVRCR creates
the worktree and starts the workspace's `local` shell session:

```sh
ovrcr workspace create --project consigint --name cleanup \
  --new-branch feature/cleanup --base main

ovrcr workspace create --project consigint --name cleanup \
  --branch feature/cleanup
```

Start an agent at the workspace root. With no command after `--`, OVRCR launches
`$SHELL`; `--label TEXT` sets the sidebar label. Bare zsh sessions load your
normal configuration, then use a compact `directory ›` prompt with a red arrow
after a failed command. Explicit commands and other shells are unchanged:

```sh
ovrcr new --project consigint --workspace cleanup --name "review cleanup" -- codex
```

Open the dashboard:

```sh
ovrcr
```

`j`/`k` select a session, `Enter` starts typing into it, `Ctrl-g` returns to
Browse, and `q` detaches and leaves everything running. Press `Space` at any time
to see the keys that apply right now.

## The dashboard

`ovrcr` with no subcommand attaches the dashboard, starting a server if none is
running. Four input modes, named in the footer:

| Mode | Enter | Leave |
| --- | --- | --- |
| Browse | default, or `Ctrl-g` | `q` detaches |
| Terminal | `Enter` on a running session | `Ctrl-g` |
| Copy | `[` | `Esc`, `q`, `Ctrl-g` |
| History | `PageUp`, or wheel up over a pane | `Esc`, `q`, `Ctrl-g` |

Terminal mode forwards keyboard and bracketed-paste input to the focused PTY and
intercepts only `Ctrl-g`. The keys below are Browse mode:

| Key | Action |
| --- | --- |
| `j` `k` Down Up | Select the next or previous visible sidebar row |
| `Enter` | Collapse or expand a selected project or workspace; type into a selected session |
| `v` | Open a second pane with the next different session |
| `Tab` `Shift-Tab` | Focus the other pane |
| `x` | Close the focused pane; its session keeps running |
| `n` | Create a terminal |
| `w` | Create a workspace |
| `a` | Register a project |
| `X` | Close the selected terminal, with confirmation |
| `p` `r` | Pause or resume the selected session |
| `[` | Freeze the current screen for copying |
| `PageUp` | Open the session's retained history |
| `t` `Ctrl-t` | Scheduled tasks |
| `:` | Command palette |
| `Space` `?` | Contextual key groups; `?` also supports arrows and Enter |
| `q` | Detach; the server and every session keep running |

The sidebar gives each session one line: a status glyph, its name, and its
model right-aligned in the provider colour. Projects are upper-case section
headers with a rule; workspaces carry a branch glyph. The glyph is blank when
idle, a green braille spinner when busy, `?` when waiting for input, `!` on a
reported error, `✓` when a response is ready, `·` after exit, and `-` until a
hook report arrives. Local shells show `$`.

`Space` and `?` open a compact popup in the bottom-right corner. In Browse,
choose a group, then an action. The available groups follow the selected row:
`t` Terminal, `w` Workspace, `p` Project, and `v` View. A terminal selection also
exposes its workspace and project. A project selection has no terminal or
workspace group. `a` Register project and `q` Detach remain available at the top.

| Sequence | Action |
| --- | --- |
| `Space t Enter` | Focus the selected running terminal; hidden when a project or workspace is selected |
| `Enter` | Collapse or expand a selected project or workspace; same as Focus on a session |
| `Space t p` / `Space t r` | Pause / resume; only the applicable action appears |
| `Space t c` / `Space t h` | Copy screen / history |
| `Space t x` | Close the selected terminal, with confirmation |
| `Space w n` | Create a terminal in the selected workspace |
| `Space w x` | Remove the selected workspace, with confirmation |
| `Space p n` | Create a workspace in the selected project |
| `Space p x` | Unregister the selected project, with confirmation |
| `Space v t` | Open scheduled tasks |

The popup names the current prefix and target. Backspace returns to the group
list; Escape closes it. With `?`, arrows browse and Enter opens the highlighted
group or runs its action. Clicking a row does the same. Invalid keys leave the
popup open. Actions awaiting a screen acknowledgement stay dimmed with a reason.
Removal retains the existing safeguards and never deletes the project repository.
Bare shortcuts in the table above still work without opening the popup.

Copy and History show their own movement and selection actions directly, so
`Space v` still sets a selection anchor in those modes. Popup actions reuse the
same hint descriptions and handlers as the command palette and direct shortcuts.

Everything else — full key tables for Copy and History, mouse and scroll
behaviour, history and clipboard bounds, the palette's forms and path pickers, and
`dashboard.toml` settings — is in the
[dashboard reference](docs/dashboard.md).

Optional [desktop notifications](docs/dashboard.md#desktop-notifications) alert
when a background managed Codex response becomes Ready. They default off; press
`N` in browse mode to toggle them for the current dashboard. Alerts identify the
terminal without including conversation content and require an active dashboard.
An independent [ready sound](docs/dashboard.md#ready-sound), toggled with `S`,
follows the same responses and also defaults off.

Managed Codex terminals also show an [unread indicator](docs/dashboard.md#unread-responses)
for their latest unreviewed Ready response. Press `R` in Browse mode or choose
**Mark reviewed** in the terminal actions. Viewing output and receiving Busy do
not clear it. Unread state survives dashboard reconnect while the server lives;
scripts can acknowledge an exact observation with `terminal mark-reviewed`.

## The CLI

The dashboard is optional. Every session is reachable from a script, and resource targets are always explicit—no command infers a target from the
current directory. New terminal names are optional and can follow app titles.

```sh
ovrcr list                                        # whole hierarchy
ovrcr project list                                # projects only
ovrcr workspace list --project consigint
ovrcr terminal list --project consigint --workspace cleanup

id=$(ovrcr terminal create --project consigint --workspace cleanup \
  -- /bin/sh)
ovrcr terminal rename "$id" "Review cleanup"   # pin a title
ovrcr terminal rename "$id" --automatic        # follow app titles
ovrcr terminal send "$id" --text "cargo test"
ovrcr terminal read "$id" --max-lines 20
ovrcr terminal close "$id"
```

Add `--json` to any resource command for machine-readable output; the mode is
always explicit and is never turned on automatically in CI or an agent
environment.

Three behaviours worth knowing before scripting against it:

- `send` respects bracketed-paste mode and then sends Enter, which `--no-submit`
  omits. A successful send means the bytes were written, not that the program was
  ready or finished. `read` returns the current screen, so a read straight after a
  send can show the earlier one; read again.
- Read commands never start a server. They never migrate the registry. With no
  server running, project and workspace queries read `config.toml.sqlite3` after
  migration. They read the preserved `config.toml` if the database is absent or
  still empty and uninitialized after an interrupted import. A foreign, damaged,
  or incompatible database is an error, not a TOML fallback. Terminal lists include
  retained rows without starting their processes. `new`, `terminal create`,
  `terminal reopen`, `terminal acknowledge-stopped`, `project add`,
  `workspace create`, and the dashboard start a server on demand.
- Requests are bounded: 30 seconds for an ordinary request, 60 for `kill`,
  `close`, and `shutdown`.

Stopping things:

```sh
ovrcr pause ID          # SIGSTOP the session's process group
ovrcr resume ID         # SIGCONT it
ovrcr kill ID           # stop it, keep the final screen
ovrcr session remove ID # drop the exited record
ovrcr terminal reopen ID # fresh shell in the same row
ovrcr shutdown          # stop a server with no live processes; --kill also stops them
```

The [CLI reference](docs/cli-reference.md) has the full command list and aliases,
JSON record shapes, removal guards, the exact signal sequence `kill` uses, and a
copy-pasteable transcript against a disposable repository.

## Agent reporting

Claude Code 2.1.267 can report observed activity, current context, partial token
usage, and estimated cost through a supervised invocation:

```sh
ovrcr agent setup claude --print --settings ~/.claude/settings.json
ovrcr new --project demo --workspace hooks --name agent -- ovrcr agent run --provider claude -- claude
ovrcr session usage SESSION_ID
```

Review and merge the printed settings before launching. OVRCR never edits provider
settings. See [Claude Code setup](docs/claude-code-setup.md) for composition,
diagnostics, removal, and supported invocation limits, and the
[support matrix](docs/agent-reporting-support.md) for acceptance evidence.

Other harness integrations remain planned. Existing generic activity/context and
legacy Claude adapters remain available for unbound sessions; their separate
contracts are documented in [agent reporting](docs/agent-reporting.md).

## Scheduled tasks

OVRCR can run scheduled Pi agents in fresh Git worktrees or scratch directories.
Task definitions, run history, and transcripts are persistent. The scheduler uses
three concurrent slots by default and gives each run a one-hour timeout.

See [scheduled tasks](docs/scheduled-tasks.md) for the CLI commands, background
service installation, scheduling rules, and retained-work cleanup.

## Configuration and environment

| Path | Default |
| --- | --- |
| Registry / config | `~/Library/Application Support/ovrcr/config.toml` on macOS, `$XDG_CONFIG_HOME/ovrcr/config.toml` (usually `~/.config/ovrcr`) on Linux |
| Project/workspace database | The full registry path with `.sqlite3` appended. Default `config.toml` therefore uses `config.toml.sqlite3`, not `config.sqlite3`. |
| Dashboard settings | `dashboard.toml` beside `config.toml` |
| Scheduled tasks | `config.tasks` beside `config.toml` |
| Server socket | `$XDG_RUNTIME_DIR/ovrcr/server.sock` on Linux, `$TMPDIR/ovrcr-UID/ovrcr/server.sock` on macOS and wherever `XDG_RUNTIME_DIR` is unset |
| Server log | `server.log` beside the socket |

`OVRCR_CONFIG` still names the `config.toml` path. Dashboard settings and
scheduled-task storage are derived from that path as before. After the first
server start, project and workspace records live in the `.sqlite3` file. The
server writes that database only and never rewrites `config.toml`. Keep
dashboard settings in `dashboard.toml`.

| Variable | Effect |
| --- | --- |
| `OVRCR_CONFIG` | Registry path; selects an isolated instance |
| `OVRCR_SOCKET` | Server socket path; selects an isolated server |
| `OVRCR_DASHBOARD_CONFIG` | `dashboard.toml` path |

Set `OVRCR_CONFIG` and `OVRCR_SOCKET` together for a test fixture or a second
instance. OVRCR creates a missing socket directory with mode 700 and refuses to
start when an existing one is a symlink or owned by another user. It never changes
the permissions of a directory it did not create. The socket file itself is always
mode 700, and that is what gates connections, so other users cannot reach the
server even from a shared directory.

These are read by the binary but exist for the integration suite and unusual
deployments; ordinary use needs none of them:

| Variable | Effect |
| --- | --- |
| `OVRCR_SERVER_EXECUTABLE` | The binary a command runs as `server` when it starts one on demand (default: the running executable) |
| `OVRCR_KILL_GRACE_MS` | The server's grace period before SIGKILL for kill, close, and `shutdown --kill` (default 5000) |
| `OVRCR_REQUEST_TIMEOUT_MS` | The client's bound on one request round trip (default 30000, or 60000 for kill, close, and shutdown) |
| `OVRCR_ENV_FILE` | An environment file the server loads before starting; the installed service points it at the file given to `service install` |
| `OVRCR_PI_EXECUTABLE` | The Pi binary used by scheduled tasks |

## Upgrading a running server

A running server keeps its original version after you rebuild, and the client
refuses to talk to a mismatched wire protocol, for example:

```
protocol version mismatch: the server speaks version 2 but this client speaks
version 3; stop the old server with `ovrcr shutdown --kill` (or restart the
installed service) and retry
```

Stop live sessions or run `ovrcr shutdown --kill`, then launch the updated binary.
Retained exited or stopped rows do not block `ovrcr shutdown`. The CLI never
stops an old server automatically and never restores its lost PTYs.

The first start of this binary against an existing `config.toml` imports project
and workspace records into that path with `.sqlite3` appended, in one transaction.
Later starts do not import again after the migration commits. The original TOML
is left unchanged for recovery. If import fails or is interrupted before commit,
the next server start can retry; it never publishes a partially imported registry.

An older binary on the same `OVRCR_CONFIG` still reads that leftover TOML, which
does not include projects or workspaces added after the import.

## Troubleshooting

A server that a command started in the background writes its output to
`server.log` beside the socket. When startup fails, the command reports the exit
status and the last lines of that log, which is where a corrupt or incompatible
project database, an unusable socket directory, or a damaged task store shows up.
If `config.toml.sqlite3` contains foreign data or uses an unsupported schema,
startup and offline project/workspace queries fail without falling back to
`config.toml`. An empty, uninitialized database left by an interrupted import
can be retried on startup; offline inspection reads the preserved TOML without
writing the database. Run `ovrcr server` in the foreground to watch startup output.

Compare a client against a long-running server with `ovrcr --version`, which
prints both the package and the protocol version.

## How it works

A single server process owns everything with state: the project/workspace
database, the Git worktrees, the PTYs, and the sessions. Clients — the dashboard and every CLI
command — connect over a private Unix socket and speak a versioned binary
protocol. The server is authoritative: a dashboard renders what the server has
confirmed rather than predicting it, which is why a paused row appears only after
a session refresh and why input is revoked the moment focus or geometry changes.

There is no async runtime. A synchronous dispatcher reads frames, a single writer
owns each socket, and per-session threads pump PTY output into bounded queues. When
a queue cannot keep up, output is coalesced into a per-session screen refresh;
control responses and lifecycle events are never dropped, and the dashboard is
disconnected explicitly if one of those cannot be delivered.

The Cargo workspace has five packages:

| Package | Owns |
| --- | --- |
| `ovrcr-protocol` | Shared wire types, validation, and framing |
| `ovrcr-terminal` | Terminal parsing, encoding, and screen/history primitives |
| `ovrcr-runtime` | The server, PTY and session ownership, registry persistence, Git worktrees, task execution |
| `ovrcr-tui` | Dashboard state, input routing, rendering, task UI |
| `ovrcr` (root) | CLI and client, reporting, service integration, optional GUI helper |

`ovrcr-runtime` and `ovrcr-tui` both depend on the protocol and terminal
primitives and never on each other; the root package composes them and keeps its
facades thin.

[`AGENTS.md`](AGENTS.md) states the invariants a change has to preserve —
ownership and lock order, the spawn/registration boundary, input-permission rules,
and the evidence each kind of claim requires. Read it before editing, rather than
inferring the rules from this summary.

## Development

`just` with no arguments lists every recipe:

```sh
just verify    # fmt-check, check, lint, test
just run       # cargo run -p ovrcr --
just restart   # stop a leftover local server, then start the dashboard
```

Or with Cargo directly — always name the package, since the workspace has five:

```sh
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets --all-features
cargo run -p ovrcr --
```

While iterating, run the owning package's tests instead:

```sh
cargo test -p ovrcr-tui --lib
cargo test -p ovrcr --test tui
```

The headless PTY wrapper exercises rendered terminal state, input and paste,
resize, mouse selection, detach, reattach, and bounded cleanup:

```sh
cargo test -p ovrcr --test terminal_acceptance -- --nocapture
```

Give every live fixture its own `OVRCR_CONFIG`, `OVRCR_SOCKET`, and temporary
workspace, so a test cannot reach your real server.

CI runs one `checks` job on macos-latest: format, clippy, the full workspace
test suite with all features (including the GUI helper), and doctests run
sequentially using one build cache. Linux-specific behavior is not checked by CI.

For changes to the dashboard or terminal handling, the
[macOS GUI helper](docs/gui-helper.md) runs the real dashboard in a native window
against a disposable demo server. Agents doing that check should follow the
[computer-use testing guide](docs/testing-computer-use.md).

## Contributing

Issues and pull requests are welcome at
[github.com/xlyk/ovrcr](https://github.com/xlyk/ovrcr).

Before you write code, read [`AGENTS.md`](AGENTS.md). It is the contributor guide
for this repository — crate boundaries, the ownership and lifecycle invariants, how
much testing evidence a change needs, and how to report it. Work on a branch, keep
unrelated cleanup out of the diff, and run `just verify` before opening a pull
request. Coding agents have extra rules there, including an `rtk` prefix on every
shell command; the commands in this README are written plain for humans.

A pull request should state the problem, the behaviour change, the exact commands
you ran to verify it, and anything still unverified.

## Roadmap and limits

Shipped: split panes, historical scrollback, keyboard copy mode, pause and resume,
agent activity hooks, context usage accounting, mouse forwarding, and retained
session rows that reopen in a fresh shell after a server restart.

Not shipped, with priorities and dates undecided:

- [ ] Native provider / agent conversation resume.
- [ ] Multiple dashboards connected to one server. **Deferred.**

Know the current limits before relying on it: one server and one attached
dashboard, a workload tested at 50 live sessions, and live PTYs plus in-memory
history. Detaching reconnects to a surviving PTY. A server crash loses those
PTYs and that history; reattaching is not crash recovery. Identity, title, kind,
and workspace stay in the store so you can reopen a fresh shell in the same row.
Agent resume is unavailable. Tests simulate boot-identity changes while reading
native boot IDs; they do not reboot the machine.

## License

OVRCR is released under the MIT license. See [`LICENSE`](LICENSE) for the full
text.

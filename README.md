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
terminal and the current `ctx -` field.

Detaching leaves the server, PTYs, and child process groups running. Run
`ovrcr` again to reattach and rebuild the selected terminal from its current
screen. Reattach works while the original dashboard is gone; the MVP does not
restore a PTY after a server crash or reboot.

## Session lifecycle

List sessions, stop a process group, and remove an exited record:

```sh
ovrcr list
ovrcr kill SESSION_ID
ovrcr session remove SESSION_ID
```

`kill` sends SIGTERM to the whole managed process group and uses SIGKILL after
the grace period when members remain. A session stays in the hierarchy after
exit until `session remove` is requested, so its final screen remains
available.

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
$ bounded_shutdown() { ovrcr shutdown --kill >/dev/null 2>&1 & p=$!; i=0; while kill -0 "$p" 2>/dev/null && [ "$i" -lt 150 ]; do sleep 0.1; i=$((i + 1)); done; kill "$p" 2>/dev/null || true; wait "$p" 2>/dev/null || true; }
$ cleanup() { bounded_shutdown; test ! -e "$OVRCR_SOCKET"; rm -rf "$d"; }
$ trap cleanup EXIT
$ git -C "$d" init -b main
$ git -C "$d" config user.name OVRCR
$ git -C "$d" config user.email ovrcr@example.invalid
$ printf 'fixture\n' > "$d/README"
$ git -C "$d" add README && git -C "$d" commit -m initial
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

## MVP limits

OVRCR uses blocking I/O and threads, one dashboard, and supports the tested
50-session scale. It retains only each session's current terminal screen.
There is no split-pane layout, historical scrollback, copy mode, pause state,
agent-specific status, context accounting, terminal mouse forwarding, or
multiple dashboards. Live PTYs and session metadata are in memory; a server
crash or reboot loses them. Reattach connects to a surviving PTY. Restore,
which would recreate a lost PTY or resume an agent conversation, is deferred.

---
name: ovrcr
description: Use when driving the ovrcr CLI to register a project, create a workspace, open a terminal, or start an interactive Claude Code, Codex, Pi, or Oh My Pi session and type a prompt into it. Triggers include ovrcr, a new worktree workspace, terminal send, terminal keystroke, or an interactive agent session.
---

# OVRCR

Drive the installed `ovrcr` binary. One server owns every session. Every command names its project and workspace; the current directory is never a target.

Use the `ovrcr` on `PATH` (`command -v ovrcr`, then `ovrcr --version`). A `cargo run` from a checkout can speak a different protocol than the server that is already running. Leave `OVRCR_CONFIG` and `OVRCR_SOCKET` unset so commands reach that server. Pass `--json` on any command whose output you will parse. Run `ovrcr <command> --help` for flags this page does not need.

The full contract is `docs/cli-reference.md` in the ovrcr checkout.

`automatic_local_terminals` is one global preference in `dashboard.toml`:

- `on` creates a `local` shell for every newly provisioned workspace.
- `off` skips automatic shells, including in the default-branch workspace.
- `default_branch_only` creates one only in the project's detected default-branch workspace. This is the default when no choice is saved.

Cycle and save it with Browse `L`, or edit `dashboard.toml`. The saved choice survives restart; the running server reads it on the next provisioning request. Changing it leaves existing terminals and agents untouched. Explicit terminal and agent launches work under every option.

## 1. Resolve the project

```sh
ovrcr project list --json
ovrcr project get NAME --json
```

Done when the JSON names the repo. Register a repository only when the user asked to add one:

```sh
ovrcr project add NAME /absolute/repo --workspace-root /absolute/worktrees
```

Registration creates the protected root workspace on the detected default branch. Whether it also starts a `local` shell follows `automatic_local_terminals` in `dashboard.toml` (default **default branch only**).

## 2. Create a workspace

A new branch needs `--base`. Read the repository's default branch and pass that name. An existing local branch uses `--branch` and takes no `--base`. The workspace's name is the branch, slashes included.

```sh
ovrcr workspace create --project NAME --new-branch temp/topic --base DEFAULT --json
ovrcr workspace create --project NAME --branch existing/branch --json
ovrcr workspace get --project NAME --branch temp/topic --json
```

Done when `workspace get` returns `path` and `branch`. Creation starts a separate `local` shell only when `automatic_local_terminals` is `on` (the default is **default branch only**, so feature workspaces do not get one automatically). Start a terminal explicitly with `ovrcr new` when you need a shell.

## 3. Open a terminal

```sh
ovrcr new --project NAME --workspace BRANCH --name LABEL --json -- /bin/sh
```

`terminal create` takes the same arguments. The program and its arguments go after `--`. With no program, OVRCR launches `$SHELL`. The JSON field `id` is the session id. Omit `--name` to let the title follow the app.

Done when `terminal list --json` shows that id with `phase` `running`.

## 4. Start an interactive agent

```sh
ovrcr new --project NAME --workspace BRANCH --name LABEL --json -- \
  "$(command -v ovrcr)" agent run PROVIDER -- "$(command -v BIN)"
```

`PROVIDER` is `claude`, `codex`, `pi`, or `omp`. `BIN` is that provider's executable. Pass the absolute binaries from `command -v`; the server runs this argv.

For Claude Code this launch was exercised end to end, including a prompt sent after the composer appeared:

```sh
ovrcr new --project NAME --workspace BRANCH --name LABEL --json -- \
  "$(command -v ovrcr)" agent run claude -- "$(command -v claude)" --permission-mode dontAsk
```

Flags that keep a fresh Claude launch eligible for reporting: `--model`, `--permission-mode`, `--agent`, `--agents`, `--settings`, `--setting-sources`, `--system-prompt`, `--append-system-prompt`, `--name` / `-n`, `--strict-mcp-config`, `--verbose`, and permission-bypass flags. A fresh launch may take one prompt after a second `--`.

`--print` / `-p` is the one-shot mode. It prints, exits, and the screen reports `agent admission unavailable`. An interactive session stays at the composer.

Done when `terminal read ID` shows the composer. Claude's composer is the `❯` prompt. A trust dialog or other menu is not done. Paste cannot move a menu selection. Answer a menu with Keystrokes only when the user asked for that answer (see below); otherwise stop and show the user that screen.

## 5. Send a prompt

Send only after the composer is on screen:

```sh
ovrcr terminal send ID --text 'Reply with exactly these two words and nothing else: hello world. Do not use tools.'
ovrcr terminal read ID --max-lines 40
```

Send writes the text, then Enter. `--no-submit` omits Enter. When the program has asked for bracketed paste, the text is pasted and Enter is written after the paste. That is how a prompt is submitted. Arrow keys and other control sequences are text in that paste, so this command is for a line the program should receive, then Enter.

`{"ok":true}` means the bytes were written. Read the screen again for the assistant's reply. The reply is the line after the echoed prompt. In the Claude run above, the screen showed the prompt and then `hello world`, with `phase` still `running` and `activity` `idle`.

Done when the reply is visible, or when you stop because the screen is a menu, the session has exited, or the composer is still waiting with no reply. Show that screen.

## Press a key

`terminal keystroke` writes one key, as the keyboard would. It is never a paste:

```sh
ovrcr terminal keystroke ID :j:
ovrcr terminal keystroke ID :enter:
ovrcr terminal read ID --max-lines 40
```

`:j:` then `:enter:` approves a menu that opens with No highlighted. Keys are one character (`:j:` and `:J:` differ) or a lowercase name: `enter`, `escape`, `tab`, `backspace`, `space`, `colon`, `up`, `down`, `left`, `right`, `home`, `end`, `pageup`, `pagedown`, `insert`, `delete`, `f1`–`f12`, with optional `ctrl-`, `alt-`, `shift-` prefixes in that order (`:ctrl-c:`, `:alt-down:`, `:ctrl-shift-enter:`). `shift-` is only for names. A bad name exits 2 and writes nothing. `Conflict` means the Dashboard is focused on that session, or it is paused, exited, or was replaced; show the user instead of retrying. `{"ok":true}` means the key was written, not handled; read the screen to see what the program did.

## Leave live work in place

A new workspace and its `local` shell stay up. Close a terminal, remove a workspace, or shut the server down only when the user asked. Live sessions and a dirty worktree block `workspace remove`. The root workspace cannot be removed.

```sh
ovrcr terminal close ID
ovrcr workspace remove --project NAME --branch BRANCH
```

## When a command fails

- `OVRCR server is not running`: project and workspace reads can continue offline. Kill, close, remove, send, and keystroke need the server. `new`, `terminal create`, `terminal reopen`, `terminal acknowledge-stopped`, `project add`, `workspace create`, and the dashboard start one.
- `protocol version mismatch`: the binary and the running server differ. Report the versions. Restart the server only when the user asked. `ovrcr shutdown --kill` stops their live sessions.
- `agent admission unavailable`: relaunch through `agent run` without print mode or flags outside the eligible set. The interactive composer is the session to keep.

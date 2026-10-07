# OVRCR

**Yet another place to run coding agents. This one is Rust, so it’s fast and you get to feel smug about it.**

[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg?style=flat-square)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.95%2B-orange.svg?style=flat-square)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Linux-lightgrey.svg?style=flat-square)](#requirements)

## About

The market is full of agent runtimes and terminal multiplexers. On a feature checklist they all look the same: sessions, splits, maybe a sidebar. Congratulations.

OVRCR is what you reach for when “a pile of agent PTYs on a pile of worktrees” stops being a personality trait. Register a Git repo. Create a workspace (it makes the worktree). Start Claude, Codex, Pi, Grok, Hermes, OMP, Cursor Agent, or a shell inside it. One Rust server owns every PTY — detach with `q`, reattach later, agents keep burning tokens without you watching. That’s not a bug; that’s the product.

Busy, waiting, Ready, and context occupancy come from provider hooks. A quiet terminal is not reported idle. We refuse to invent vibes from silence. Your quota chaos is still your problem; we just stop lying about which session caused it.

It is written in Rust (edition 2024, because of course it is). It also ships eight themes so you can doomscroll Appearance settings instead of the agent that is waiting on you: Catppuccin Mocha (default), Catppuccin Latte, Tokyo Night, Dracula, Gruvbox Dark, Gruvbox Light, Nord, Rosé Pine. Functionally irrelevant. Emotionally load-bearing.

Reporting depth varies by provider — see the [support matrix](docs/agent-reporting-support.md).

## Screenshots

<p align="center">
  <img src="docs/images/01-hero-dashboard.png" alt="Dashboard: projects and worktrees in the sidebar; one focused session pane" width="900">
</p>

<table>
  <tr>
    <td width="50%">
      <img src="docs/images/02-busy-vs-waiting.png" alt="Waiting vs busy — claude-why-tho waiting for input" width="100%">
    </td>
    <td width="50%">
      <img src="docs/images/03-split-panes.png" alt="Split panes — shell and Claude" width="100%">
    </td>
  </tr>
</table>


## Why this instead of tmux + coping

- **Organization is Git-shaped.** Registered projects and worktree workspaces, not a flat session list. Creating a workspace creates the branch, the worktree, and (by default) a local shell. Dirty or foreign worktrees are refused — OVRCR will not “helpfully” adopt the mess you made at 2am.
- **PTYs outlive the UI.** One server owns the process groups. Detach and reattach do not kill agents. Closing the dashboard is not the same as admitting defeat.
- **Status is reported, not inferred.** Activity and context come from provider hooks. Silence is not idle. If the agent is waiting on you, the dashboard says so — it does not pretend the model is “thinking” because the PTY went quiet.
- **Rust, so it’s fast.** Also memory-safe, also compile times that build character, also a README badge that makes strangers trust you on GitHub.
- **Themes, because of course.** Eight built-ins in Appearance (`dashboard.toml` `theme`, or the Settings pick list). Default is Catppuccin Mocha. Switch to Nord when the agents get chaotic. Switch to Dracula when they get worse. None of this fixes your quota.

## Install

Pre-1.0. No package, no brew formula, no “curl | sh” that owns your laptop. Build from source like an adult:

```sh
git clone https://github.com/xlyk/ovrcr.git
cd ovrcr
cargo build -p ovrcr --release
install -m 755 target/release/ovrcr ~/.local/bin/ovrcr
ovrcr --version
```

`ovrcr --version` prints package version and wire protocol. Client and server must match. Mismatch is how you invent ghost sessions and then blame Rust.

## Quickstart

```sh
ovrcr project add myapp ~/Code/myapp --workspace-root ~/Code/workspaces/myapp
ovrcr workspace create --project myapp --new-branch feature/cleanup --base main
ovrcr new --project myapp --workspace feature/cleanup --name review -- claude
ovrcr
```

Dashboard: `j`/`k` select, `Enter` focus, `Ctrl-g` browse, `q` detach (agents stay up; your denial stays optional).

## Features

- Sessions grouped by registered Git project and worktree workspace; create/remove refuse dirty or unregistered worktrees.
- One or two panes; Browse, Terminal, Copy, and History modes.
- PTYs survive dashboard detach. After server restart, retained rows reopen a fresh shell in place; certified Claude conversations can resume by UUID. Terminal output is not restored across server restart — the agents remember more than the scrollback does.
- 512 rows of scrollback per session; keyboard selection; OSC52 clipboard copy.
- Pause / resume via SIGSTOP / SIGCONT on the session process group (for when four agents decide to `cargo check` at once).
- Scriptable CLI with `--json` on every resource command — same sessions, zero dashboard, maximum “I’ll automate this later.”
- Optional: scheduled Pi tasks in fresh worktrees; macOS Bridge notifications for when a run goes Ready and you were busy picking a theme.
- Eight selectable themes (Catppuccin Mocha/Latte, Tokyo Night, Dracula, Gruvbox Dark/Light, Nord, Rosé Pine). Your agents will not become smarter. Your screenshots will.

## Requirements

- macOS or Linux
- Rust 1.95 or newer (edition 2024; optional GUI helper’s `eframe` sets the floor — yes, really 1.95)
- Git
- Optional: [Pi](docs/scheduled-tasks.md) for scheduled tasks; macOS [Bridge](native/bridge/README.md) for notifications
- A healthy relationship with API quotas (sold separately)

## Docs

- [Dashboard](docs/dashboard.md) — modes, keys, notifications, quota, events
- [CLI reference](docs/cli-reference.md)
- [Agent reporting](docs/agent-reporting.md) · [Claude setup](docs/claude-code-setup.md) · [Support matrix](docs/agent-reporting-support.md)
- [Scheduled tasks](docs/scheduled-tasks.md) · [Provider quota](docs/provider-quota.md)
- [`AGENTS.md`](AGENTS.md) — invariants for contributors and coding agents

## Contributing

There is no separate `CONTRIBUTING.md` yet. Start with [`AGENTS.md`](AGENTS.md) for the project’s invariants, then open an issue or PR on GitHub.

```sh
cargo test -p ovrcr
```

## License

OVRCR is licensed under the MIT License. See [`LICENSE`](LICENSE).

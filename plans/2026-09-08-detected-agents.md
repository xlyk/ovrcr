# Detected Agents and the One-Key Terminal Plan

> **Execution:** Use `superpowers:subagent-driven-development` or `superpowers:executing-plans`. Each task is one PR against `main`.

**Outcome:** Creating a terminal from the dashboard takes one hotkey and a pick: `n` opens a form whose Agent field lists the coding agents found on `PATH`, the workspace and name are prefilled, and the dashboard attaches to the new session at once.

**Baseline:** `main` after PR #28. The palette lives in `crates/ovrcr-tui/src/dashboard/palette.rs`: `:` opens a `Page::Search` over `Entry` values, Enter on "Create terminal", "Create workspace", or "Register project" opens a `Page::Form` of free-text `Field`s built by `palette_form`, and the last Enter builds a `Request` in `palette_key`. Browse-mode keys are in `key_action` in `dashboard/state.rs`; keyboard selection is session-only (`selected: Option<SessionId>`), so project and workspace rows are reached only by mouse.

**Decisions already taken:**
- Agents are detected on `PATH`; `dashboard.toml` can add or override entries. No mandatory preset configuration.
- Creation lands the user in terminal mode on the new session.

## Constraints

- Keep blocking I/O and threads. Do not add tokio or any async runtime.
- The dashboard is a client. Anything it reads from Git or the filesystem for suggestions is best-effort and must never block the event loop for more than a few tens of milliseconds; use the existing `TaskWorker` pattern in `task_tui.rs` for anything slower.
- Dashboard settings live in their own file, `dashboard.toml`, beside `config.toml`. The server rewrites `config.toml` atomically and would drop unknown tables, so never put dashboard settings there.
- Any change to a serialized type in `crates/ovrcr-protocol` bumps `PROTOCOL_VERSION` and regenerates the wire snapshot in `wire.rs`.
- OVRCR never removes a repository. The root workspace is not a worktree and is never passed to `git worktree remove` or `prune`.
- Every behavior change gets the smallest test that would have caught a regression. Skip matrices.
- Prefix every executable and pipeline stage with `rtk`. Before committing: `rtk proxy just verify` plus the task's named tests.

---

### Task 1: Detected agents and the one-key terminal



**Files:**
- Create: `crates/ovrcr-tui/src/dashboard/agents.rs`
- Create: `crates/ovrcr-tui/src/dashboard/settings.rs`
- Create: `crates/ovrcr-tui/src/dashboard/picker.rs`
- Modify: `crates/ovrcr-tui/src/dashboard/palette.rs`
- Modify: `crates/ovrcr-tui/src/dashboard/state.rs`
- Modify: `crates/ovrcr-tui/src/dashboard/render.rs`
- Modify: `crates/ovrcr-tui/src/dashboard/mod.rs`
- Modify: `src/cli/mod.rs` (pass the settings path into `run_dashboard`)
- Test: `crates/ovrcr-tui/src/dashboard/tests.rs`, `tests/tui.rs`

**Design:**
- `agents.rs`: `pub fn detect_agents(path: &OsStr, shell: Option<&OsStr>) -> Vec<AgentEntry>` walks the `PATH` entries once for the known names, in this order: `claude`, `codex`, `gemini`, `aider`, `opencode`, `pi`, `goose`, `amp`, `cursor-agent`. Each hit becomes `AgentEntry { name, argv: vec![full path], source: Detected }`. `$SHELL` is always appended as `AgentEntry { name: "shell", argv: [shell], source: Shell }` and a final `Custom` entry opens a free command field that runs through `/bin/sh -lc` as today. No `--version` probing.
- `settings.rs`: `DashboardSettings { agents: Vec<AgentOverride>, picker_roots: Vec<PathBuf>, branch_prefix: String }` loaded from `dashboard.toml` beside the registry (`OVRCR_DASHBOARD_CONFIG` overrides the path, mirroring `OVRCR_CONFIG`). A missing file yields defaults. An `AgentOverride { name, argv }` with a name matching a detected entry replaces its argv; a new name is appended before `shell`. Parse errors show in the footer and fall back to defaults; they never stop the dashboard.
- `picker.rs`: a reusable `PickList { items: Vec<PickItem>, query: String, selected: usize }` rendered under a form field, with fuzzy subsequence filtering on the label, arrow selection, Tab or Enter to accept, and typing to filter. `plans/2026-09-08-workspace-creation-picks.md` and `plans/2026-09-08-project-path-picker.md` reuse it.
- `Field` gains `kind: FieldKind` where `FieldKind::Text` is today's behavior and `FieldKind::Pick(PickList)` renders the list and stores the accepted value.
- The new-terminal form becomes: Agent (pick, detected list), Workspace (pick, all `project / workspace` pairs, defaulting to the selected session's), Name (text, default `<agent>-<n>` where `n` is the next unused number in that workspace, or `local` when the agent is `shell` and the workspace has no `local` session), Command (text, shown only when Agent is `Custom`). Label defaults to the agent name.
- `n` in browse mode opens that form directly with the pick lists prefilled. `:` still works; the palette entry reads "Create terminal (n)".
- On `Response::CreatedSession`, `palette_response` already selects the session; extend it to enter `InputMode::Terminal` so typing goes to the agent at once.
- Discoverability is the which-key plan's job (`plans/2026-09-08-which-key-popup.md`); Task 1 only registers `n` in the hint table introduced there, or adds a one-line footer hint if that plan has not landed yet.

- [ ] **Step 1: RED tests**
  1. `agents.rs` unit: a temp dir with executable stubs `claude` and `codex` on a synthetic `PATH` yields those two, then `shell`, then `Custom`; a non-executable file named `gemini` is skipped; an override for `claude` replaces its argv.
  2. `settings.rs` unit: a `dashboard.toml` with `[[agents]] name="claude" argv=["claude","--verbose"]` and `picker_roots=["~/Code"]` parses and expands `~`; an invalid file yields defaults plus an error string.
  3. `tests/tui.rs`: `n_opens_terminal_form_prefilled_for_selected_workspace`: with a session selected, press `n`, assert the form's Workspace field holds that session's `project / workspace` and the Agent list's first item is the first detected agent.
  4. `tests/tui.rs`: `created_session_enters_terminal_mode`: feed `Response::CreatedSession` for the pending request and assert `mode == InputMode::Terminal` and the selection.
  5. `tests/tui.rs`: `n_is_listed_in_the_hint_table` once the which-key plan has landed, otherwise a footer render check.

- [ ] **Step 2: Implement** the modules and the form changes above. Detection runs when the form opens, so a newly installed agent appears without restarting the dashboard.

- [ ] **Step 3: Verify** `rtk proxy cargo test -p ovrcr-tui` and `rtk proxy cargo test --test tui --test terminal_acceptance`.

**Gate:** new tests pass; existing palette tests pass unchanged except for the renamed entry label.

---

### Task 2: Documentation and the computer-use smoke check

**Files:**
- Modify: `README.md`
- Modify: `docs/testing-computer-use.md`

- [ ] **Step 1:** Document detected agents, the `n` key, and `dashboard.toml`'s `agents` table with an example in the README "Command palette" section, and add a smoke-check step to `docs/testing-computer-use.md`: press `n`, pick `shell`, and confirm typing reaches the new terminal.

**Gate:** the README transcript in "Disposable repository transcript" still runs as written.

---

## Final verification

From a clean checkout of the merged branch:

```sh
rtk proxy just verify
rtk proxy cargo test --features gui --test gui
```

Then, with a release build and an isolated `OVRCR_CONFIG` and `OVRCR_SOCKET`:

1. Open the dashboard, press `n`, pick a detected agent, and confirm the session starts in the selected workspace and the dashboard is in terminal mode.
2. Run the computer-use smoke check in `docs/testing-computer-use.md`.

# OVRCR Dashboard Creation Flow Plan

> **Execution:** Use `superpowers:subagent-driven-development` or `superpowers:executing-plans`. Complete the tasks in order; each task is one PR against `main`.

**Outcome:** Creating a terminal, a workspace, or a project from the dashboard takes one hotkey and a pick or two instead of a palette trip through free-text fields, every project has a shell in its checkout by default, and the common path lands the user inside the new session.

**Baseline:** `claude/review-fixes` (PR #28). The palette lives in `crates/ovrcr-tui/src/dashboard/palette.rs`: `:` opens a `Page::Search` over `Entry` values, Enter on "Create terminal", "Create workspace", or "Register project" opens a `Page::Form` of free-text `Field`s built by `palette_form`, and the last Enter builds a `Request` in `palette_key`. Browse-mode keys are in `key_action` in `dashboard/state.rs`; keyboard selection is session-only (`selected: Option<SessionId>`), so project and workspace rows are reached only by mouse.

**Decisions already taken:**
- Agents are detected on `PATH`; a settings file can add or override entries. No mandatory preset configuration.
- The path picker applies to the two path fields of "Register project". The workspace root is prefilled from the project name.
- Every project gets an implicit `root` workspace whose path is the repository checkout. Its `local` shell starts on registration. After a server restart the row stays visible and the shell starts lazily on the first new-terminal action. `project remove` closes the root `local` shell itself but refuses while any other session remains in `root`.

## Constraints

- Keep blocking I/O and threads. Do not add tokio or any async runtime.
- The dashboard is a client. Anything it reads from Git or the filesystem for suggestions is best-effort and must never block the event loop for more than a few tens of milliseconds; use the existing `TaskWorker` pattern in `task_tui.rs` for anything slower.
- Dashboard settings live in their own file, `dashboard.toml`, beside `config.toml`. The server rewrites `config.toml` atomically and would drop unknown tables, so never put dashboard settings there.
- Any change to a serialized type in `crates/ovrcr-protocol` bumps `PROTOCOL_VERSION` and regenerates the wire snapshot in `wire.rs`.
- OVRCR never removes a repository. The root workspace is not a worktree and is never passed to `git worktree remove` or `prune`.
- Every behavior change gets the smallest test that would have caught a regression. Skip matrices.
- Prefix every executable and pipeline stage with `rtk`. Before committing: `rtk proxy just verify` plus the task's named tests.

## Priority order

| Task | Delivers | Why this order |
| --- | --- | --- |
| 1 | Detected agents, `n` hotkey, auto-attach | TUI-only, changes the most common action |
| 2 | Which-key popup with per-key descriptions, start screen | Teaches every hotkey the later tasks add; one hint table feeds popup, palette, and footer |
| 3 | Pick lists and defaults for workspaces, `w` hotkey | TUI-only, reuses Task 1's pick-list widget |
| 4 | Path picker for project registration, `a` hotkey | TUI-only, reuses the same widget |
| 5 | Root workspace per project | Touches registry, server, removal guards; needs its own safety tests |
| 6 | Docs and GUI smoke-check update | Last, describes the finished flow |

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
- `picker.rs`: a reusable `PickList { items: Vec<PickItem>, query: String, selected: usize }` rendered under a form field, with fuzzy subsequence filtering on the label, arrow selection, Tab or Enter to accept, and typing to filter. Tasks 3 and 4 reuse it.
- `Field` gains `kind: FieldKind` where `FieldKind::Text` is today's behavior and `FieldKind::Pick(PickList)` renders the list and stores the accepted value.
- The new-terminal form becomes: Agent (pick, detected list), Workspace (pick, all `project / workspace` pairs, defaulting to the selected session's), Name (text, default `<agent>-<n>` where `n` is the next unused number in that workspace, or `local` when the agent is `shell` and the workspace has no `local` session), Command (text, shown only when Agent is `Custom`). Label defaults to the agent name.
- `n` in browse mode opens that form directly with the pick lists prefilled. `:` still works; the palette entry reads "Create terminal (n)".
- On `Response::CreatedSession`, `palette_response` already selects the session; extend it to enter `InputMode::Terminal` so typing goes to the agent at once.
- Discoverability is Task 2's job; Task 1 only registers `n` in the hint table introduced there, or adds a one-line footer hint if Task 2 has not landed yet.

- [ ] **Step 1: RED tests**
  1. `agents.rs` unit: a temp dir with executable stubs `claude` and `codex` on a synthetic `PATH` yields those two, then `shell`, then `Custom`; a non-executable file named `gemini` is skipped; an override for `claude` replaces its argv.
  2. `settings.rs` unit: a `dashboard.toml` with `[[agents]] name="claude" argv=["claude","--verbose"]` and `picker_roots=["~/Code"]` parses and expands `~`; an invalid file yields defaults plus an error string.
  3. `tests/tui.rs`: `n_opens_terminal_form_prefilled_for_selected_workspace`: with a session selected, press `n`, assert the form's Workspace field holds that session's `project / workspace` and the Agent list's first item is the first detected agent.
  4. `tests/tui.rs`: `created_session_enters_terminal_mode`: feed `Response::CreatedSession` for the pending request and assert `mode == InputMode::Terminal` and the selection.
  5. `tests/tui.rs`: `n_is_listed_in_the_hint_table` once Task 2 exists, otherwise a footer render check.

- [ ] **Step 2: Implement** the modules and the form changes above. Detection runs when the form opens, so a newly installed agent appears without restarting the dashboard.

- [ ] **Step 3: Verify** `rtk proxy cargo test -p ovrcr-tui` and `rtk proxy cargo test --test tui --test terminal_acceptance`.

**Gate:** new tests pass; existing palette tests pass unchanged except for the renamed entry label.

---

### Task 2: Which-key popup with descriptions, and a start screen

**Files:**
- Create: `crates/ovrcr-tui/src/dashboard/hints.rs`
- Create: `crates/ovrcr-tui/src/dashboard/whichkey.rs`
- Modify: `crates/ovrcr-tui/src/dashboard/state.rs`
- Modify: `crates/ovrcr-tui/src/dashboard/render.rs`
- Modify: `crates/ovrcr-tui/src/dashboard/palette.rs`
- Modify: `crates/ovrcr-tui/src/dashboard/mod.rs`
- Test: `crates/ovrcr-tui/src/dashboard/tests.rs`, `tests/tui.rs`

**Design:**
- `hints.rs`: one function, `pub fn key_hints(dashboard: &Dashboard) -> Vec<HintGroup>`, builds the whole table from current state. `HintGroup { title, hints: Vec<KeyHint> }` and `KeyHint { key: &'static str, name: &'static str, description: String, enabled: bool, action: HintAction }`. This table is the single source of truth: the popup renders it, the palette shows each entry's key and description as secondary text, and the footer prints a compressed line of the enabled keys.
- **Description rules**, which are the acceptance criteria:
  1. Name the target. The description substitutes the selected session, workspace, or project name, so `w` reads "Create a worktree and branch under consigint and start its local shell" and `x` reads "Stop agent (#12) in auth-handoff and remove its record. Asks for confirmation."
  2. State the consequence. Destructive or side-effecting keys say what happens and whether a confirmation follows. Detach says "the server and every session keep running".
  3. Disabled keys stay listed, dimmed, with the reason in place of the description: "Resume: not paused", "Close: no session selected".
- **Groups from browse mode:** Create (`n`, `w`, `a`), Session (`Enter`, `p`, `r`, `x`, `[`, `PageUp`), View (`t`, `:`, `o` once the operator exists, `?`), Dashboard (`q`). Copy and history modes get their own tables listing `hjkl`, `0`, `$`, `g`, `G`, `v`, `y`, and the exits. Terminal mode has no popup; `Ctrl-g` is its only chord and the footer already names it.
- **Leader and help keys.** Space in browse, copy, or history mode opens the popup; the next key runs that entry and closes it, so `Space n` and bare `n` are the same action. `?` opens the same popup in a browsing state where arrows move and Enter runs the highlighted entry, which also makes it usable from the GUI helper by mouse. Escape closes. `whichkey.rs` holds the popup state (`pending_leader: bool`, `browsing: Option<usize>`) and its key handling; `key_action` in `state.rs` consults it first while it is open.
- **Rendering.** A centered popup, borrowing the palette's `Clear` and block layout, with one column per group when the width allows, otherwise stacked. Each row is key, name, description. On a pane narrower than 100 columns the rows show key and name only, and the highlighted row's description moves to a detail line at the bottom, so nothing is truncated silently. `x` is a new browse key that opens the existing close confirmation for the selected session; it exists so the Session group has a close entry.
- **Start screen.** When the hierarchy has no projects, the terminal pane shows a short list instead of an empty screen: `a` register project, `:` palette, `?` help, and the config and socket paths. When a workspace with no sessions is selected by mouse, the pane shows "press n to start a terminal here". Both are drawn from the same hint table so their wording matches the popup.

- [ ] **Step 1: RED tests**
  1. `hints` unit: with a running session selected in `consigint / auth-handoff`, the Create group's `w` description contains `consigint`, the Session group's `x` description contains the session name and id and the word "confirmation", `r` is disabled with reason "not paused"; with nothing selected the Session group's entries are disabled with "no session selected".
  2. `tests/tui.rs`: `space_then_n_opens_the_terminal_form`: press Space, assert the popup is drawn with the Create group, press `n`, assert the popup is gone and the terminal form is open.
  3. `tests/tui.rs`: `question_mark_popup_is_browsable`: press `?`, Down twice, Enter, assert the third enabled entry's action ran.
  4. `tests/tui.rs`: `narrow_popup_moves_description_to_detail_line` on an 80-column `TestBackend`: descriptions are absent from the rows and the highlighted one appears on the last popup line.
  5. `tests/tui.rs`: `empty_hierarchy_shows_start_screen` and `palette_entries_show_keys_and_descriptions`.

- [ ] **Step 2: Implement** `hints.rs`, `whichkey.rs`, the `x` key, the palette's secondary text, the compressed footer line, and the start screen.

- [ ] **Step 3: Verify** `rtk proxy cargo test -p ovrcr-tui` and `rtk proxy cargo test --test tui --test terminal_acceptance`.

**Gate:** new tests pass; existing browse-mode key tests pass unchanged, since bare hotkeys keep working without the leader.

---

### Task 3: Workspace creation with pick lists and defaults

**Files:**
- Modify: `crates/ovrcr-tui/src/dashboard/palette.rs`
- Modify: `crates/ovrcr-tui/src/dashboard/state.rs`
- Create: `crates/ovrcr-tui/src/dashboard/git_hints.rs`
- Test: `crates/ovrcr-tui/src/dashboard/tests.rs`, `tests/tui.rs`

**Design:**
- The form becomes: Project (pick, defaulting to the selected session's project), Name (text), Branch mode (toggle field: `new` or `existing`, Space or Left/Right flips it), Branch (for `new`: text prefilled with `<branch_prefix><name>`, updating live until edited; for `existing`: pick list of local branches), Base (for `new` only: text prefilled with the project's default branch).
- `git_hints.rs` provides `local_branches(repo) -> Vec<String>` via `git for-each-ref refs/heads --format=%(refname:short)` and `default_branch(repo) -> String` by reading `.git/refs/remotes/origin/HEAD` or `packed-refs`, falling back to `main` if `refs/heads/main` exists, then `master`, then the current `HEAD`. Both take the repository path, which the dashboard obtains once per palette open through `Request::Inspect` (the `Inventory` response carries the registry with repo paths). Results are cached per project for the life of the palette. The subprocess is bounded by a 2-second `wait` with a timeout thread; on failure the field falls back to free text with a footer note.
- `w` in browse mode opens the form with Project prefilled. The palette entry reads "Create workspace (w)".
- After `Response::Ok` for `CreateWorkspace`, the hierarchy event that follows carries the new `local` session; select it and enter terminal mode, matching Task 1.

- [ ] **Step 1: RED tests**
  1. `git_hints` unit against a temp repository: `local_branches` lists `main` and a created branch; `default_branch` returns `main`, and returns the `origin/HEAD` target when a remote is configured.
  2. `tests/tui.rs`: `w_opens_workspace_form_with_project_and_derived_branch`: press `w` with a session selected, type a name, assert Branch shows `feature/<name>`, flip the mode, assert Branch becomes a pick list.
  3. `tests/tui.rs`: `workspace_creation_attaches_to_its_local_shell`.

- [ ] **Step 2: Implement.** The `branch_prefix` default is `feature/`, overridable in `dashboard.toml`.

- [ ] **Step 3: Verify** as in Task 1.

**Gate:** new tests pass; `git_lifecycle` unchanged.

---

### Task 4: Path picker for project registration

**Files:**
- Modify: `crates/ovrcr-tui/src/dashboard/picker.rs`
- Modify: `crates/ovrcr-tui/src/dashboard/palette.rs`
- Modify: `crates/ovrcr-tui/src/dashboard/state.rs`
- Test: `crates/ovrcr-tui/src/dashboard/tests.rs`, `tests/tui.rs`

**Design:**
- `FieldKind::Path(PathPicker)`: the field holds the typed text; the list under it shows the children of the directory named by the text up to its last separator, filtered by fuzzy subsequence on the last segment, directories only, hidden entries excluded unless the segment starts with `.`. `~` expands. Tab accepts the highlighted entry and appends `/` so the next segment can be typed; Enter accepts the text as typed. Entries whose directory contains `.git` carry a marker and sort first. Listing is a single `read_dir` of one directory, so it stays synchronous; a directory with more than 500 entries is truncated with a count.
- When the field is empty, the list shows `picker_roots` from `dashboard.toml` (default `~/Code`, `~/src`, `~` when they exist) so the common case is two Tabs.
- The form becomes: Repository (path picker), Name (text, prefilled with the repository's basename once the repository is chosen, editable), Workspace root (path picker, prefilled with `<config dir>/workspaces/<name>` and updated while Name changes until edited). The server already canonicalizes and validates both paths; the dashboard only makes them easier to type.
- `a` in browse mode opens the form. The palette entry reads "Register project (a)".
- After registration, Task 5 gives the project a root `local` shell; until then, select the project's first session if any.

- [ ] **Step 1: RED tests**
  1. `picker` unit against a temp tree: typing `~/Co` lists `Code`; Tab yields `~/Code/`; a child with `.git` is marked and sorted first; a hidden directory appears only when the segment starts with `.`.
  2. `tests/tui.rs`: `a_opens_project_form_with_roots_and_derives_name_and_root`.

- [ ] **Step 2: Implement.**

- [ ] **Step 3: Verify** as in Task 1.

**Gate:** new tests pass; `cli` and `resource_cli` unchanged.

---

### Task 5: A root workspace for every project

**Files:**
- Modify: `crates/ovrcr-protocol/src/registry.rs`
- Modify: `crates/ovrcr-runtime/src/server/mod.rs`
- Modify: `crates/ovrcr-runtime/src/git.rs`
- Modify: `src/cli/resources.rs`, `src/cli/output.rs`
- Modify: `crates/ovrcr-tui/src/dashboard/render.rs`
- Modify: `README.md`
- Test: `crates/ovrcr-protocol/src/registry.rs`, `crates/ovrcr-runtime/src/server/tests.rs`, `tests/server_lifecycle.rs`, `tests/resource_cli.rs`, `tests/git_lifecycle.rs`

**Design:**
- The name `root` is reserved: `Registry::add_workspace` and `validate_name` reject it for user workspaces.
- `Registry::workspace_records(project) -> Vec<WorkspaceRecord>` returns a synthesized `root` record first (`path` = repository, `branch` read from `.git/HEAD` as `ref: refs/heads/<name>` or the short hash when detached) followed by the persisted records. Nothing about `root` is written to `config.toml`. `Registry::workspace_path(project, name)` resolves `root` to the repository for `create_session_locked`; `snapshot_from_state` and the CLI's `workspace list` and `workspace get` use `workspace_records` so `root` appears everywhere with `terminal_count`.
- `add_project` starts the root `local` shell exactly as `create_workspace` does for a new worktree, with the same `PartialFailure` reporting when `$SHELL` is unset. After a server restart no session exists; the row still renders, and the new-terminal form from Task 1 defaults the name to `local` for a `shell` agent when `root` has none.
- `remove_workspace` with `root` returns `InvalidRequest` ("the root workspace is the repository checkout"). `inspect_worktree` and `remove_worktree` are never called for it.
- `remove_project`: if any session in `root` other than one named `local` exists, `SessionsRemain`; otherwise close the `local` session with `close_terminal` semantics (terminate, refresh, remove record), then proceed with the existing workspace-remaining check and registry write. The hierarchy event after removal drops the project.
- `render.rs`: the `root` row is drawn with a distinct marker and the branch from `.git/HEAD` so it reads as the checkout, not a worktree.

- [ ] **Step 1: RED tests**
  1. `registry.rs`: `workspace_records` puts `root` first with the repository path; `add_workspace("root")` is rejected.
  2. `server_lifecycle.rs`: `project_registration_starts_a_root_shell`: after `AddProject`, `List` shows `fixture / root / local` running with `pid` set and its cwd equal to the repository (read from the shell with `pwd`).
  3. `server_lifecycle.rs`: `root_workspace_cannot_be_removed_and_project_remove_closes_its_shell`: `RemoveWorkspace { name: "root" }` is `InvalidRequest`; with an extra agent session in `root`, `RemoveProject` is `SessionsRemain`; after closing that session, `RemoveProject` succeeds, the `local` process group is gone, and the repository directory still exists with its `.git`.
  4. `git_lifecycle.rs`: `root_workspace_never_reaches_git_worktree_commands`: the `git` invocations recorded through a `PATH` shim during project removal contain no `worktree remove` or `worktree prune`.
  5. `resource_cli.rs`: `workspace list --json` includes `root` with `branch` and `terminal_count`.

- [ ] **Step 2: Implement.** No wire type changes are needed because `root` is synthesized as ordinary `WorkspaceRecord` and `WorkspaceSummary` values; if a field is added after all, bump `PROTOCOL_VERSION` and regenerate the snapshot.

- [ ] **Step 3: Verify** `rtk proxy cargo test --workspace`.

**Gate:** all new tests pass; `removes_clean_registered_worktree_and_preserves_branch` and the removal-gate tests are unchanged.

---

### Task 6: Documentation and the computer-use smoke check

**Files:**
- Modify: `README.md`
- Modify: `docs/testing-computer-use.md`

- [ ] **Step 1:** Rewrite the "Command palette" section around the hotkeys: `n`, `w`, `a`, Space or `?` for the help popup, then `:` for everything else. Document detected agents, `dashboard.toml` with its three keys and an example, the path picker keys, the derived defaults, and the root workspace rules including what `project remove` does to its shell.
- [ ] **Step 2:** Add a step to the smoke check: press `a`, pick the demo repository through the picker, confirm the new project shows a `root / local` row, press `n`, pick `shell`, and confirm typing reaches it.

**Gate:** the README transcript in "Disposable repository transcript" still runs as written.

---

## Final verification

From a clean checkout of the merged branch:

```sh
rtk proxy just verify
rtk proxy cargo test --features gui --test gui
```

Then, with a release build and an isolated `OVRCR_CONFIG` and `OVRCR_SOCKET`:

1. `ovrcr project add` a repository and confirm `ovrcr list` shows `root / local` running in the checkout.
2. Open the dashboard, press `n`, pick a detected agent, and confirm the session starts in the selected workspace and the dashboard is in terminal mode.
3. Press `w`, type a name, confirm the branch reads `feature/<name>` and the base is the repository's default branch, submit, and confirm the new workspace's `local` shell is selected.
4. Press `a`, reach a repository with two Tabs from a configured root, and confirm the name and workspace root were derived.
5. `ovrcr workspace remove --project X --name root` is refused; `ovrcr project remove X` closes the root shell and leaves the repository intact.
6. Run the computer-use smoke check in `docs/testing-computer-use.md`.

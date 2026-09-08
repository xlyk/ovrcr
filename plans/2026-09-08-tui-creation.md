# OVRCR Dashboard Creation Flow Plan

> **Execution:** Use `superpowers:subagent-driven-development` or `superpowers:executing-plans`. Complete the tasks in order; each task is one PR against `main`.

**Outcome:** Creating a terminal, a workspace, or a project from the dashboard takes one hotkey and a pick or two instead of a palette trip through free-text fields, every project has a shell in its checkout by default, and the common path lands the user inside the new session. Tasks 7 through 9 then complete the dashboard's roadmap: two panes side by side, mouse input forwarded to applications that ask for it plus wheel scrolling of the pane, and explicit session restore after server loss.

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
| 6 | Docs and GUI smoke-check update | Describes the creation flow |
| 7 | Split panes | Changes the view contract; mouse forwarding builds on its `pane_rects` |
| 8 | Terminal mouse forwarding and pane wheel scrolling | Reuses `pane_rects` and the history snapshot |
| 9 | Session restore | Durable state and a Claude adapter; independent of 7 and 8 but last because it is the largest |

Tasks 7 through 9 fold in `plans/2026-09-05-split-panes.md`, `plans/2026-09-05-terminal-mouse-forwarding.md`, and `plans/2026-09-05-session-restore.md`. Those files keep the per-step code sketches and are marked superseded; the decisions and gates below are authoritative where they differ.

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

### Task 7: Split panes

**Source:** `plans/2026-09-05-split-panes.md` for the step-level sketches. Decisions below are final.

**Files:**
- Modify: `crates/ovrcr-protocol/src/wire.rs` (`PaneTarget`, `DashboardView`, `Request::SetView`, `revision` on `Response::Screen`, `ServerEvent::Output`, `ServerEvent::ScreenDirty`; bump `PROTOCOL_VERSION`, regenerate the snapshot)
- Modify: `crates/ovrcr-runtime/src/server/{mod,dispatch,connections,outbound}.rs`
- Modify: `crates/ovrcr-tui/src/dashboard/{mod,state,input,render,event_loop}.rs`
- Test: `tests/tui.rs`, `tests/server_lifecycle.rs`, `tests/terminal_acceptance.rs`, protocol and runtime unit test modules

**Decisions:**
- One or two panes, always side by side, a one-column `│` separator, integer width split with the extra column on the right, each pane keeping its two metadata rows. No split tree, horizontal split, ratios, drag handles, or third pane.
- Browse mode: `v` opens the second pane with the next different visible session and focuses it ("No other visible session to split" when none); repeated `v` is a no-op; Tab and Shift-Tab switch focus; `x` closes the focused pane when there are two and expands the survivor, and closes the selected session's record when there is one (the Task 2 meaning). These join the which-key table from Task 2 with descriptions naming the pane's session.
- Browsing the sidebar replaces the focused pane's session; selecting a session already shown in the other pane focuses that pane instead, so one PTY never receives two sizes. A browse-mode click in a pane focuses it and sends no bytes.
- Enter attaches the focused, ready, live pane; Ctrl-g returns to browse. No broadcast input. A pane awaiting its snapshot refuses input with "Pane is loading; retry input". Exited panes keep their final screen.
- Each split pane needs at least 20 columns and one row; when the right area cannot fit both, keep both slots but draw and subscribe only the focused pane at full width with "split hidden: terminal too small", restoring automatically. Never submit a zero size.
- Closing, replacing, or hiding a pane unsubscribes its session; the server parser keeps consuming and the PTY keeps its last size. The focused visible pane's size is the default for new sessions.
- Detach and reattach start in browse mode with one pane; layout is not persisted. Closing a pane never kills, removes, or recreates a session.
- The operator row from `plans/2026-09-08-operator-agent.md` is an ordinary session for pane purposes; `o` assigns it to the focused pane.

**Wire and server contract:**

```rust
pub struct PaneTarget { pub session: SessionId, pub size: TerminalSize }
pub struct DashboardView { pub revision: u64, pub panes: Vec<PaneTarget>, pub focused: Option<SessionId> }
// Request::SetView { view: DashboardView }
// revision: u64 added to Response::Screen, ServerEvent::Output, ServerEvent::ScreenDirty
impl DashboardView { pub fn validate(&self) -> Result<(), String> }
```

`validate` rejects zero revision, more than two panes, duplicate sessions, zero rows or columns, and a focus outside the list; empty requires no focus, nonempty requires focus. `ServerState.selected` becomes `view: Mutex<Option<DashboardView>>`; `last_user_selection` from the operator plan follows the focused pane. `DispatchMessage::SetView { owner, request_id, view, completion }` validates every session before resizing any PTY, resizes only changed geometries, captures each screen, and calls `DashboardSink::replace_view(&view, request_id, screens) -> bool`, which atomically discards obsolete output and dirty flags and queues one `Screen` per visible pane followed by one `Ok`. Preflight queue room for all of them or disconnect the dashboard rather than drop lifecycle frames. A failed second resize reports `PartialFailure` and disconnects that dashboard, preserving processes. `Select` and `Resize` remain as adapters: `Select` builds a singleton view at the next revision; `Resize` is accepted only for a singleton view and answers `InvalidRequest: use SetView for split geometry` otherwise. Output is emitted as `(revision, session, bytes)` only for committed view members; dirty keys are `(revision, SessionId)`; on a dirty event the client re-requests its whole view at a new revision.

**Dashboard contract:**

```rust
pub struct PaneState { pub session: Option<SessionId>, pub parser: vt100::Parser, pub size: TerminalSize, pub desired_size: TerminalSize, pub snapshot_installed: bool, pub ready: bool, pub error: Option<String> }
pub struct PaneRects { pub pane_index: usize, pub metadata: Rect, pub terminal: Rect }
pub fn pane_rects(area: Rect, pane_count: usize, focused: usize) -> Vec<PaneRects>;
// Dashboard: panes: Vec<PaneState>, focused_pane: usize, view_revision: u64
// focused_session(), split_pane(), focus_pane(i), close_focused_pane(),
// view_request(area, request_id) -> Result<Option<ClientMessage>>, apply_screen(revision, session, size, bytes)
```

`view_request` increments the revision with checked arithmetic and marks visible targets unready; `apply_screen` accepts only the current revision for a visible assigned session and replaces only that pane's parser; the matching `Ok` marks panes ready. `select_session` stays the sidebar entry point. Copy mode and history bind to the focused pane's session and reset when its assignment changes.

- [ ] **Step 1: RED tests.** Protocol: `split_view_validation_rejects_ambiguous_targets`, `split_view_frames_round_trip`. Runtime: `set_view_publishes_two_ordered_snapshots_then_ok`, `set_view_overflow_disconnects_instead_of_dropping_lifecycle_frames`, `resize_is_rejected_for_a_split_view`. TUI: `v_splits_to_the_next_visible_session_and_focuses_it`, `selecting_the_other_panes_session_focuses_it`, `pane_rects_at_120x40_are_39x36_and_40x36`, `narrow_terminal_hides_the_split_and_restores_it`, `input_reaches_only_the_focused_ready_pane`, `stale_screen_cannot_replace_a_reassigned_pane`. Acceptance: two real sessions render side by side through the outer PTY, keystrokes reach only the focused one, a burst on both recovers both screens, detach and reattach preserve both PTYs.
- [ ] **Step 2: Implement** in the source plan's order: view contract, ordered snapshots and overflow recovery, pane-local focus and modes, exact rectangles and per-pane resize, then the acceptance run.
- [ ] **Step 3: Verify** `rtk proxy cargo test --workspace` and the acceptance suite.

**Gate:** the six acceptance criteria in the source plan hold: independent PTY sizes (39×36 and 40×36 at 120×40), focused-only input with per-pane cursor and paste modes, snapshots precede increments per revision, narrow terminals never send zero sizes, close/hide/detach/reattach preserve PTYs, and the existing sidebar, restoration, and 50-session gates still pass.

---

### Task 8: Terminal mouse forwarding and pane wheel scrolling

**Source:** `plans/2026-09-05-terminal-mouse-forwarding.md` for the step-level sketches and byte tables. Decisions below are final, plus the wheel-scrolling addition that closes the gap between that plan and historical scrollback.

**Files:**
- Modify: `crates/ovrcr-tui/src/dashboard/{input,state,event_loop,mod,terminal_guard}.rs`, `render.rs` where the pane rectangle is consumed
- Create: `tests/support/mouse_app.rs` (a raw fixture that enables a tracking mode and echoes received bytes)
- Test: `tests/tui.rs`, `tests/terminal_acceptance.rs`

No wire change; mouse bytes travel in the existing `Request::Input`.

**Decisions:**
- Forward only to the focused, ready, live pane's session, only in terminal mode, only when that session's parser reports a tracking mode (1000, 1002, 1003, or X10) it has requested. Encoding enablement alone forwards nothing.
- Forward left, middle, and right presses; releases in 1000/1002/1003; drag in 1002/1003; unpressed motion only in 1003; wheel up/down/left/right in 1000/1002/1003; X10 gets the three presses only. Shift, Alt, and Control modifiers except in X10. No Shift-click bypass.
- Coordinates are computed only after bounds checks against `pane_rects` from Task 7; metadata rows, separators, footer, sidebar, and cells beyond the parser's size are rejected. Columns are cells, never character indices.
- A press owns its session until release or cancellation; up to three held buttons; duplicate motion at the same cell is suppressed. Leaving the rectangle emits a release at the last valid position. Before Ctrl-g, selection replacement, resize, or detach, held buttons are released with the old encoding and old session id, and that cleanup frame is written before the selection or resize request.
- Outer mouse capture is on in browse mode or while forwarding is eligible, reconciled after output and snapshots as well as keys, using the existing enable and disable commands. Focus reporting is on for the dashboard lifetime; `FocusLost` finishes gestures and suppresses forwarding until `FocusGained`.
- A parsed protocol change clears held state without sending old-protocol releases. During snapshot resynchronization or a pending resize, forwarding is disabled until the matching acknowledgement. Exited or removed sessions never receive mouse input.
- **Pane wheel scrolling (new):** in browse mode, or in terminal mode when the focused application has not requested a tracking mode, a wheel-up over a pane opens the historical view (`begin_history_request`) for that pane's session positioned at the captured tail, and further wheel ticks page it one row per tick; a wheel-down at the newest row closes history and returns to the live pane. When the application has requested tracking, the wheel is forwarded to it instead. The which-key table's History entry gains "or scroll the wheel over the pane".

**Interfaces:**

```rust
pub fn encode_mouse(event: MouseEvent, mode: vt100::MouseProtocolMode, encoding: vt100::MouseProtocolEncoding) -> Option<Vec<u8>>;
// Dashboard: mouse: MouseForwarding { held: [Option<HeldMouse>; 3], last_motion, pending_cleanup: Option<ClientMessage> },
//   mouse_focused: bool, mouse_ready: bool, mouse_awaiting: Option<MouseAwaiting>
// pub fn mouse_capture_required(&self) -> bool; pub fn cancel_mouse_gesture(&mut self);
// pub fn take_mouse_cleanup(&mut self) -> Option<ClientMessage>; fn terminal_mouse_enabled(&self) -> bool; fn reconcile_mouse_protocol(&mut self);
```

`None` from `encode_mouse` means the event is not representable; it never returns a truncated sequence. `pending_cleanup` holds at most one frame of at most three releases. Every path that writes a request drains cleanup first.

- [ ] **Step 1: RED tests.** Unit: exact byte tables for every supported mode and encoding, all three buttons, all wheel axes, combined modifiers, SGR coordinates above 223, legacy cell 222 accepted and 223 rejected. TUI: `browse_click_in_pane_focuses_without_bytes`, `press_outside_rectangle_is_not_clamped`, `leaving_the_rectangle_releases_at_last_valid_cell`, `ctrl_g_releases_held_buttons_before_switching`, `protocol_change_clears_held_state`, `wheel_over_a_pane_without_tracking_opens_history_at_the_tail`, `wheel_down_at_newest_row_returns_to_live`, `wheel_is_forwarded_when_tracking_is_enabled`. Acceptance: through the outer PTY with `tests/support/mouse_app.rs`, send SGR press, drag, release, and wheel and assert the exact relative bytes; then the same gestures against a plain shell show no bytes and the wheel opens history.
- [ ] **Step 2: Implement** encoder, gesture ownership through `pane_rects`, capture and focus reconciliation, snapshot and resize gating, then the wheel rule.
- [ ] **Step 3: Verify** `rtk proxy cargo test --workspace`, the acceptance suite, and a manual pass in a real terminal with Vim `:set mouse=a` and the computer-use guide for the native gate.

**Gate:** application requests control forwarding; browse ownership, Ctrl-g, keyboard and paste encoding, and terminal restoration are unchanged; held buttons never migrate between sessions; the wheel scrolls a plain shell's history and drives Vim when it asks.

---

### Task 9: Session restore after server loss

**Source:** `plans/2026-09-05-session-restore.md` for the state machine table, storage rules, and step sketches. Decisions below are final.

**Files:**
- Create: `crates/ovrcr-runtime/src/restore.rs`
- Modify: `crates/ovrcr-runtime/src/config.rs` (extract `write_atomic(path, bytes, mode) -> Result<(), SaveFailure>`), `crates/ovrcr-runtime/src/server/{mod,dispatch,startup}.rs`, `crates/ovrcr-runtime/src/session/mod.rs`
- Modify: `crates/ovrcr-protocol/src/wire.rs` and `session.rs` (`restore_mode` on `CreateSessionRequest`, `Request::SavedSessions`, `Request::RestoreSession`, `ack_orphans_gone` on `RemoveSession`, `Response::SavedSessions`, `incarnation` on `SessionSummary`, `SessionPhase::Recoverable { reason }`; bump `PROTOCOL_VERSION`, regenerate the snapshot)
- Modify: `src/cli/{args,mod,output}.rs`, `crates/ovrcr-tui/src/dashboard/{state,render}.rs`
- Test: `tests/server_lifecycle.rs`, `tests/cli.rs`, `tests/tui.rs`, `crates/ovrcr-runtime/src/restore.rs` unit tests

**Decisions:**
- Opt-in at creation: `new --restore-mode relaunch`, or `new --restore-mode claude --conversation-id UUID -- claude`. Default sessions, workspace `local` shells, and the operator stay ephemeral. No retrospective recording.
- Saved intent is the canonical executable (resolved on the server's PATH at creation), exact argv bytes, the canonical workspace path, the mode, and an attempt state. No environment, credentials, screen contents, or transcripts. File mode 0600; argv never appears in list output or errors, and CLI help warns that argv can contain secrets.
- Storage is `<config-filename>.sessions.toml` beside the registry, written with the shared atomic writer, guarded by a lifetime `flock` on `<config-filename>.sessions.lock` acquired before the registry loads or the socket binds; two servers on one config are an ownership conflict. Version 1 schema, unknown fields rejected, at most 512 records and 64 KiB of argv per record; malformed, oversized, or duplicate files fail closed without replacement.
- Session ids are reserved durably before every creation, including ephemeral ones, so a saved id is never reused after restart.
- Startup never launches anything. `session saved` lists records without starting a server. `session restore ID --expected-incarnation N` starts the server if needed and restores exactly one record; there is no automatic or batch restore. An interrupted attempt is `MayBeRunning` and requires `--ack-orphans-gone`, an operator assertion that old descendants are gone; OVRCR never signals a saved PID. `kill` on an uncertain saved-only record refuses with inspection instructions. `session remove` of an uncertain record also needs the acknowledgement. Saved records block workspace removal; `shutdown --kill` refuses unresolved uncertain records; ordinary shutdown may leave safely stopped saved records.
- A restore creates a new PTY, PID, group, incarnation, and start time. Events carry the incarnation and mismatches are discarded. The dashboard shows saved-only rows as "not running" with the restore reason, never an elapsed time since 1970, and the which-key Session group gains `R` "Restore this saved session" with the reason as its disabled text.
- Claude Code is the only adapter: initial argv `[claude, --session-id, uuid]`, restore `[claude, --resume, uuid]`, one input argv element only, a five-second `--help` probe requiring both option tokens before committing intent, refusal when `CLAUDE_CODE_SKIP_PROMPT_HISTORY` is truthy, and a successful spawn reported as "resume launched", never "conversation restored".

**Interfaces:**

```rust
pub struct RestoreFile { pub version: u32, pub next_id: u64, pub records: Vec<SavedSession> }
pub struct SavedSession { pub id: SessionId, pub incarnation: u64, pub project: String, pub workspace: String, pub name: String, pub label: String, pub cwd: Vec<u8>, pub argv: Vec<Vec<u8>>, pub mode: RestoreMode, pub attempt: AttemptState }
pub enum RestoreMode { Relaunch, Claude { conversation_id: String } }
pub enum AttemptState { MayBeRunning, Stopped }
pub fn load(path: &Path) -> Result<RestoreFile>; pub fn save(path: &Path, data: &RestoreFile) -> Result<(), SaveFailure>;
pub fn acquire_owner(config: &Path) -> Result<File>; pub fn decode_spec(record: &SavedSession) -> Result<SessionSpec>;
pub fn launch_argv(record: &SavedSession, restoring: bool) -> Result<Vec<OsString>>; pub fn check_claude(executable: &Path, cwd: &Path) -> Result<()>;
// ServerState::restore_session(id, expected: u64, ack: bool) -> Result<SessionSummary>; saved_sessions() -> Vec<SavedSessionSummary>
```

Lock order is mutation, then sessions, then restore; the dispatcher persists exits under the restore mutex only, after applying the event, and never takes the mutation lock. `SaveFailure { source, replaced }` distinguishes a pre-rename failure (nothing changed) from a post-rename sync failure (durable mutations freeze until restart).

- [ ] **Step 1: RED tests.** Unit: round trip with non-UTF-8 argv, rejection of each malformed shape, oversize refusal, lock conflict between two owners, `launch_argv` for both modes, `check_claude` against a fake `claude` that prints or omits the tokens. Server: `id_reservation_survives_restart`, `intent_is_durable_before_spawn`, `interrupted_attempt_blocks_until_acknowledged`, `restore_reuses_id_and_increments_incarnation`, `stale_incarnation_events_are_discarded`, `saved_records_block_workspace_removal_and_kill_shutdown`. CLI: `session saved` offline, `session restore` with wrong and right incarnation, `--ack-orphans-gone` on remove. TUI: recoverable rows render "not running" and the reason. Crash acceptance: start a saved relaunch session through the real binary, SIGKILL the server, prove the descendant survives and restore is refused, acknowledge, restore, and prove a new PID and incarnation.
- [ ] **Step 2: Implement** in the source plan's order: durable storage and owner lock, intent before spawn, explicit reconciliation and gates, CLI and adapter, then the crash acceptance and documentation.
- [ ] **Step 3: Verify** `rtk proxy cargo test --workspace` plus the crash acceptance run; measure the 50-session lifecycle gate before and after, since each allocation, intent, stop, and remove adds one fsync transaction.

**Gate:** the acceptance list in the source plan: saved intent survives process loss, no old process or screen is ever presented as restored, every launch has durable pre-spawn intent, ambiguous attempts stay recoverable without an automatic duplicate, crash tests prove surviving descendants are refused until acknowledged, and provider identity is unchanged across restore.

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
6. Press `v` on a session and confirm two panes render with independent sizes and that typing reaches only the focused one; press `x` to close it.
7. In Vim with `:set mouse=a` inside a pane, click, drag, and wheel; then over a plain shell, wheel up and confirm the history view opens at the tail.
8. Start a session with `--restore-mode relaunch`, kill the server with SIGKILL, run `ovrcr session saved`, restore it with the acknowledgement, and confirm a new PID and incarnation.
9. Run the computer-use smoke check in `docs/testing-computer-use.md`.

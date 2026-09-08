# Workspace Creation with Pick Lists and Defaults Plan

> **Execution:** Use `superpowers:subagent-driven-development` or `superpowers:executing-plans`. Each task is one PR against `main`.

**Outcome:** Creating a workspace from the dashboard is `w`, a name, and Enter: the project is picked from a list, the branch mode is a toggle, the new branch name derives from the workspace name, the base defaults to the repository's default branch, and the dashboard attaches to the workspace's local shell.

**Baseline:** `main` after PR #28. The palette lives in `crates/ovrcr-tui/src/dashboard/palette.rs`: `:` opens a `Page::Search` over `Entry` values, Enter on "Create terminal", "Create workspace", or "Register project" opens a `Page::Form` of free-text `Field`s built by `palette_form`, and the last Enter builds a `Request` in `palette_key`. Browse-mode keys are in `key_action` in `dashboard/state.rs`; keyboard selection is session-only (`selected: Option<SessionId>`), so project and workspace rows are reached only by mouse.

**Decisions already taken:**
- Git lookups on the dashboard are best-effort, cached per palette open, bounded to two seconds, and fall back to free text.
- The branch prefix defaults to `feature/` and is overridable in `dashboard.toml`.

**Dependencies:** Depends on the `PickList` widget and `dashboard.toml` loader from `plans/2026-09-08-detected-agents.md`.

## Constraints

- Keep blocking I/O and threads. Do not add tokio or any async runtime.
- The dashboard is a client. Anything it reads from Git or the filesystem for suggestions is best-effort and must never block the event loop for more than a few tens of milliseconds; use the existing `TaskWorker` pattern in `task_tui.rs` for anything slower.
- Dashboard settings live in their own file, `dashboard.toml`, beside `config.toml`. The server rewrites `config.toml` atomically and would drop unknown tables, so never put dashboard settings there.
- Any change to a serialized type in `crates/ovrcr-protocol` bumps `PROTOCOL_VERSION` and regenerates the wire snapshot in `wire.rs`.
- OVRCR never removes a repository. The root workspace is not a worktree and is never passed to `git worktree remove` or `prune`.
- Every behavior change gets the smallest test that would have caught a regression. Skip matrices.
- Prefix every executable and pipeline stage with `rtk`. Before committing: `rtk proxy just verify` plus the task's named tests.

---

### Task 1: Workspace creation with pick lists and defaults



**Files:**
- Modify: `crates/ovrcr-tui/src/dashboard/palette.rs`
- Modify: `crates/ovrcr-tui/src/dashboard/state.rs`
- Create: `crates/ovrcr-tui/src/dashboard/git_hints.rs`
- Test: `crates/ovrcr-tui/src/dashboard/tests.rs`, `tests/tui.rs`

**Design:**
- The form becomes: Project (pick, defaulting to the selected session's project), Name (text), Branch mode (toggle field: `new` or `existing`, Space or Left/Right flips it), Branch (for `new`: text prefilled with `<branch_prefix><name>`, updating live until edited; for `existing`: pick list of local branches), Base (for `new` only: text prefilled with the project's default branch).
- `git_hints.rs` provides `local_branches(repo) -> Vec<String>` via `git for-each-ref refs/heads --format=%(refname:short)` and `default_branch(repo) -> String` by reading `.git/refs/remotes/origin/HEAD` or `packed-refs`, falling back to `main` if `refs/heads/main` exists, then `master`, then the current `HEAD`. Both take the repository path, which the dashboard obtains once per palette open through `Request::Inspect` (the `Inventory` response carries the registry with repo paths). Results are cached per project for the life of the palette. The subprocess is bounded by a 2-second `wait` with a timeout thread; on failure the field falls back to free text with a footer note.
- `w` in browse mode opens the form with Project prefilled. The palette entry reads "Create workspace (w)".
- After `Response::Ok` for `CreateWorkspace`, the hierarchy event that follows carries the new `local` session; select it and enter terminal mode, matching `plans/2026-09-08-detected-agents.md`.

- [ ] **Step 1: RED tests**
  1. `git_hints` unit against a temp repository: `local_branches` lists `main` and a created branch; `default_branch` returns `main`, and returns the `origin/HEAD` target when a remote is configured.
  2. `tests/tui.rs`: `w_opens_workspace_form_with_project_and_derived_branch`: press `w` with a session selected, type a name, assert Branch shows `feature/<name>`, flip the mode, assert Branch becomes a pick list.
  3. `tests/tui.rs`: `workspace_creation_attaches_to_its_local_shell`.

- [ ] **Step 2: Implement.** The `branch_prefix` default is `feature/`, overridable in `dashboard.toml`.

- [ ] **Step 3: Verify** `rtk proxy cargo test -p ovrcr-tui` and `rtk proxy cargo test --test tui --test terminal_acceptance`.

**Gate:** new tests pass; `git_lifecycle` unchanged.

---

### Task 2: Documentation and the computer-use smoke check

**Files:**
- Modify: `README.md`
- Modify: `docs/testing-computer-use.md`

- [ ] **Step 1:** Document `w`, the branch-mode toggle, and the derived defaults in the README "Command palette" section, and add a smoke-check step to `docs/testing-computer-use.md`: press `w`, type a name, confirm the derived branch and base, submit, and confirm the local shell is selected.

**Gate:** the README transcript in "Disposable repository transcript" still runs as written.

---

## Final verification

From a clean checkout of the merged branch:

```sh
rtk proxy just verify
rtk proxy cargo test --features gui --test gui
```

Then, with a release build and an isolated `OVRCR_CONFIG` and `OVRCR_SOCKET`:

1. Press `w`, type a name, confirm the branch reads `feature/<name>` and the base is the repository's default branch, submit, and confirm the new workspace's `local` shell is selected.
2. Run the computer-use smoke check in `docs/testing-computer-use.md`.

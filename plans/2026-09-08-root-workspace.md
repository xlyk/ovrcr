# A Root Workspace for Every Project Plan

> **Execution:** Use `superpowers:subagent-driven-development` or `superpowers:executing-plans`. Each task is one PR against `main`.

**Outcome:** Every registered project has a `root` workspace whose path is the repository checkout, with a `local` shell started on registration, so a project is usable the moment it is added.

**Baseline:** `main` after PR #28. The palette lives in `crates/ovrcr-tui/src/dashboard/palette.rs`: `:` opens a `Page::Search` over `Entry` values, Enter on "Create terminal", "Create workspace", or "Register project" opens a `Page::Form` of free-text `Field`s built by `palette_form`, and the last Enter builds a `Request` in `palette_key`. Browse-mode keys are in `key_action` in `dashboard/state.rs`; keyboard selection is session-only (`selected: Option<SessionId>`), so project and workspace rows are reached only by mouse.

**Decisions already taken:**
- `root` is synthesized, never persisted, and never passed to any `git worktree` command.
- Its `local` shell starts on registration; after a server restart the row stays visible and the shell starts lazily on the first new-terminal action.
- `project remove` closes the root `local` shell itself but refuses while any other session remains in `root`.

**Dependencies:** Independent of the other dashboard plans; the new-terminal form from `plans/2026-09-08-detected-agents.md` defaults the name to `local` when `root` has none.

## Constraints

- Keep blocking I/O and threads. Do not add tokio or any async runtime.
- The dashboard is a client. Anything it reads from Git or the filesystem for suggestions is best-effort and must never block the event loop for more than a few tens of milliseconds; use the existing `TaskWorker` pattern in `task_tui.rs` for anything slower.
- Dashboard settings live in their own file, `dashboard.toml`, beside `config.toml`. The server rewrites `config.toml` atomically and would drop unknown tables, so never put dashboard settings there.
- Any change to a serialized type in `crates/ovrcr-protocol` bumps `PROTOCOL_VERSION` and regenerates the wire snapshot in `wire.rs`.
- OVRCR never removes a repository. The root workspace is not a worktree and is never passed to `git worktree remove` or `prune`.
- Every behavior change gets the smallest test that would have caught a regression. Skip matrices.
- Prefix every executable and pipeline stage with `rtk`. Before committing: `rtk proxy just verify` plus the task's named tests.

---

### Task 1: A root workspace for every project



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
- `add_project` starts the root `local` shell exactly as `create_workspace` does for a new worktree, with the same `PartialFailure` reporting when `$SHELL` is unset. After a server restart no session exists; the row still renders, and the new-terminal form from `plans/2026-09-08-detected-agents.md` defaults the name to `local` for a `shell` agent when `root` has none.
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

### Task 2: Documentation and the computer-use smoke check

**Files:**
- Modify: `README.md`
- Modify: `docs/testing-computer-use.md`

- [ ] **Step 1:** Document the root workspace rules in the README "Register a project" and "Session lifecycle" sections, including what `project remove` does to its shell, and add a smoke-check step to `docs/testing-computer-use.md`: register the demo repository and confirm a `root / local` row appears.

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
2. `ovrcr workspace remove --project X --name root` is refused; `ovrcr project remove X` closes the root shell and leaves the repository intact.
3. Run the computer-use smoke check in `docs/testing-computer-use.md`.

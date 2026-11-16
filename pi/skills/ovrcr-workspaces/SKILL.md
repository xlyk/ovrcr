---
name: ovrcr-workspaces
description: Inspect or register Projects, create or reuse Git Workspaces, and launch requested Terminals or managed Agents with verified readiness.
---

# Prepare an OVRCR Workspace

## Installation and scope

This procedure ships with OVRCR for macOS and Linux; no checkout or personal
skill installation is needed. Use the inherited matching `OVRCR_EXECUTABLE` and
instance environment. Check `"$OVRCR_EXECUTABLE" --version`; consult its subcommand
`--help` for flags. The shared CLI guide supplies command syntax and the existing
`automatic_local_terminals` policy in the instance's `dashboard.toml`.

## Resolve before provisioning

1. Establish what was requested: inspection, registration, a new or existing
   branch, and a Terminal, managed Agent, or no explicit launch. Use captured
   identities only for the captured target; a named request must resolve against
   current inventory. Ask for information needed for that action if missing;
   inspection needs no branch base or launch choice. No launch request means no
   explicit launch. Selecting this procedure grants no extra permission.
2. Read `project list --json`, then `project get NAME --json` and
   `workspace list --project NAME --json` for the intended registration.
   Compare canonical repository paths, not just Project names. For an
   unregistered repository, inspect the user-specified path with read-only Git:
   `git -C PATH rev-parse --show-toplevel`, `git -C PATH status --short --branch`
   and `git -C PATH worktree list --porcelain`. Inspection alone neither registers
   a Project nor launches a session.
3. Match the intended Workspace's current branch and path. Reuse a suitable
   registered checkout rather than creating an equivalent Workspace. A branch
   label is not an identity: keep the Project, Workspace id and verified path.
   For ambiguous matches, list candidate ids and paths and ask which path to use.
   For a detached checkout, report its path and HEAD and ask whether that exact
   checkout is intended. After resolution use `workspace get --project NAME
   --path PATH` and `terminal create --project NAME --path PATH`; never choose
   the first match or change Git to make a label match.

Proceed only when the target and requested effects are explicit.

## Create only the requested resources

1. If registration was requested and no equivalent registration exists, run
   `project add NAME /absolute/repo --workspace-root /absolute/worktrees`.
   Registration accepts the repository's current checkout; it does not switch
   Root to the default branch. Root cannot be removed. New Root launches are
   refused while it is off the repository's default branch. Surface that refusal;
   never switch Root's branch or put delegated feature edits there.
2. For a requested new branch, require the user's explicit base (branch or commit)
   and verify it in the intended repository. Then run
   `workspace create --project NAME --new-branch BRANCH --base BASE --json`.
   Do not infer a base from the default branch, current directory, or selection.
   For an existing local branch with no suitable registered Workspace, use
   `workspace create --project NAME --branch BRANCH --json` without `--base`.
   If Git reports an existing checkout, inspect and resolve it rather than
   forcing or creating a second copy.
3. Re-read the Project and Workspace. Verify the id, path, current branch and Git
   HEAD against the requested source before launching. Registration and Workspace
   creation may start `local` shells under the saved automatic-local-terminal
   policy; preserve that policy and reuse an appropriate existing shell. A
   request for inspection or no explicit launch does not authorize an additional
   Terminal or Agent. Do not change settings to suppress or multiply shells.

If a command refuses or reports partial/unknown effects, re-read inventory and
Git state for the original target. Report what exists and what remains blocked;
never blindly replay registration, creation, or launch.

## Launch and verify

1. Inspect `terminal list --json` for suitable existing sessions in the verified
   Workspace before creating one. Reuse only the requested kind and provider;
   a display label or shell named after an Agent does not make it one. Preserve
   session id and current run. A Busy Agent or one awaiting human input is not a
   free session for new work.
2. For an explicitly requested Terminal, use `terminal create --project NAME
   --path PATH --name LABEL -- /bin/sh` (or the user's requested command).
   For an explicitly requested managed Agent, use the shared CLI guide's managed
   route: `terminal create --project NAME --path PATH --name LABEL --
   "$OVRCR_EXECUTABLE" agent run PROVIDER -- /absolute/provider-executable`.
   Keep the existing managed trust policy; do not add permission answers.
3. Re-read inventory and require the returned id/run to be a running Workspace
   session in that path with the requested launch kind. For a Terminal, read
   its live screen to verify the requested program. For an Agent, also read
   `terminal read ID` or `terminal screen ID` for the actual interactive composer
   and check provider/Reporter observations when available. Process creation,
   a label, a shell placeholder, or successful input write alone is not Agent
   readiness. Unavailable reporting stays unknown, not Ready.
4. Surface any unexpected trust, setup, login, or approval screen with the
   session identity and exact question. Leave it unanswered. Send an initial
   task only when requested and the composer is verified; then inspect the
   provider's response, not just command echo. Claude Code and Codex support
   `terminal transcript ID --json`; Pi and Oh My Pi use terminal read/screen.
   Ready is a response-cycle observation, not task success. Use the coordinator
   skill for subsequent bounded delegation and completion watches.

Finish with the scope and Project / Workspace id / path / branch / session id /
run mapping. Distinguish reused, created, running-but-unverified, ready for input,
and blocked resources, citing the observed readiness or concrete missing decision.
Leave resources available; provisioning does not authorize stop, archive, removal,
branch changes, or periodic checks.

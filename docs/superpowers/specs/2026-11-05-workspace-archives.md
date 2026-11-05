# Compressed workspace archives

Product choices confirmed with Kyle on 2026-11-05. This document records the
workspace feature design; compressed archive creation and restoration are not
implemented by the accompanying session-name fix.
The implementation details below are proposed requirements based on the current
lifecycle and recovery code.

## Purpose

Archived work should remain recoverable without occupying a checkout or blocking
new sessions. Session names are display metadata, while session and workspace IDs
identify retained work.

Closing a session releases its name for creation and keeps the workspace. A
separate, explicit Archive workspace operation saves the checkout in compressed
storage and removes the working checkout after verification. Closing the last
session does not automatically archive the workspace.

## Archive contents and lifetime

Keep archives locally under the owning instance directory, in private storage,
until the user explicitly deletes them. No automatic expiration or upload.
Keep each archive under a unique identity independent of its display name.

The archive contains the working files, including uncommitted, untracked and
ignored files; the Git objects and state needed to reconstruct the checkout;
and versioned metadata linking the original project, workspace and retained
sessions. Preserve file modes and symlinks. Refuse unsupported file types before
removing anything.

A tar of a linked worktree alone is insufficient: its `.git` file points to
administration data outside the checkout. Include an independent Git bundle or
equivalent Git recovery data, the pinned HEAD and necessary index state. A
submodule checkout likewise needs its own recoverable Git state; refuse archival
until every affected checkout can be restored.

Provider conversation references remain in the existing SessionStore. Files
inside the checkout travel with it. External provider stores remain owned by the
provider, as with current session retention; the workspace archive does not
promise an independent backup of external conversation history or credentials.

## Removal and recovery

Reuse the Server lifecycle owner, workspace ownership checks and SessionStore.
The repository-root workspace cannot be archived. Running sessions, uncertain
process ownership, active task runs and protected delegation transitions block
archival until their existing lifecycle requirements are satisfied.

Persist archive intent, write to a private staging location, construct and verify
a restore candidate, and confirm that the source still matches the snapshot
before removing the checkout. A failed snapshot or verification keeps the
checkout. Publish the complete archive before Git removal. Record enough durable
progress to recover a crash between archive publication, Git removal and metadata
commit. Never report a partial removal as rolled back. A snapshot alone must not
certify process exit.

Archive the associated session rows without deleting their identities or
conversation references. Keep archive inventory discoverable after restart and
when the original checkout is absent. Interrupted archival must expose its actual
state and a recovery action.

## Restore

Restore the saved files into a separate checkout under a fresh workspace identity
and a new branch name. In particular, a newer workspace using the old branch must
not block restoration or be modified. Preserve the archived branch's original
commit in the restored Git history, together with its saved working/index changes.

Do not overwrite an existing directory. Validate archive paths and symlinks before
extraction, and verify the reconstructed checkout before publishing it. Keep the
original archive after restore so a failed or interrupted attempt remains retryable.
Repeated restore requests resolve to the same restored workspace; they cannot
move its sessions into another checkout or replace a live restored run.

Restore retained sessions as stopped rows without launching commands. Preserve
their IDs and names while updating workspace placement and affected working
directories. Provider references to files inside the original checkout require
adapter-supported relocation and validation; an unsupported relocation must
remain a visible recovery gap rather than silently starting a new conversation.

## Delivery and proof

Add explicit archive, list and restore operations to the CLI and Dashboard using
the existing lifecycle worker and SQLite authority. Update help and references
with the actual implemented format, location, interruption handling and limits.

Acceptance must cover dirty/untracked/ignored files, modes and symlinks, Git/index
state, restart, a reused original branch, supported provider-history relocation,
source changes during capture, failed archive verification, failed Git removal,
failed metadata commit, unsafe/corrupt archive entries and interrupted restore.
Prove the native Dashboard archive and restore flow from the reviewed checkout
with isolated resources and retained screenshots/accessibility evidence.

These gates remain open until implemented and run. The session-name regression
tests do not establish workspace archival or restoration correctness.

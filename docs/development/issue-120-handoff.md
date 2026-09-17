# Issue #120 development handoff

Development: [#120](https://github.com/xlyk/ovrcr/issues/120).
Independent native acceptance: [#131](https://github.com/xlyk/ovrcr/issues/131).
Base: `e241f505efec53217d7229d70aea3f388c39e028`.
Use the commit containing this handoff on `implement-task-120`; record its full
`git rev-parse HEAD` before launching. No native acceptance is claimed here.

## Behavior

- Confirmed workspace removal archives stopped records, preserving IDs, titles,
  original project/workspace names, working directories and provider references.
- Live or ownership-uncertain records block removal, including uncertain archives.
  Existing Git and scheduled-task ownership protections remain in force.
- Recorded provider history inside a worktree blocks deletion, including ignored
  files. Records never cause provider-history deletion.
- Registry removal and archive transitions share one SQLite transaction. Writes
  are prepared before Git removal. Git failure rolls metadata back. Commit failure
  after Git removal returns partial failure and retains the original records;
  recovery requires inspecting Git/storage, not assuming the worktree survived.
- Project removal still requires removing workspaces first; it does not cascade
  into session records. Unarchived rows remain selectable after either removal.
  Reopening a missing directory fails without choosing another directory.
- Protocol 20 adds `SessionSummary.cwd` on top of main's protocol 19 Codex
  references; all clients must use the same revision.
  SQLite remains schema 5. No dependencies or provider adapters were added.

## Merge verification (2026-09-17)

Merged main `31ee5ba127f23c5ea5f7c18e807e788a028f6ec3` into PR #135.
Both branches had independently used protocol 19; the combined wire format is
protocol 20. The recorded-history removal guard now includes Codex, with the
ignored-history regression exercising both Pi and Codex references.

- `SHELL=/bin/sh cargo test --workspace --all-targets --all-features --no-fail-fast`:
  **973 passed, 0 failed, 20 ignored**.
- All-target/all-feature typecheck, Clippy with warnings denied, formatting and
  diff checks passed. Node reporting extension tests: **18 passed**.
- Main fixed the backpressure test's handling of unrelated events; the failure
  recorded below no longer blocks this revision.
- Logs: `/tmp/ovrcr-120-evidence/merge/`. Native acceptance and independent review
  remain open; ignored cases are not counted as passed.

## Original implementation verification

Tested on macOS 26.5.2, Rust 1.98.0, Git 2.50.1, Node 22.23.2.
Logs, including failed attempts, are at `/tmp/ovrcr-120-evidence/` on the
implementation host. Tests used isolated config, sockets, Git repositories and
owned processes. The temporary baseline checkout was removed after testing.

| Command | Result |
| --- | --- |
| `cargo check --workspace --all-targets --all-features` | Passed |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | Passed |
| `cargo fmt --all -- --check`; `git diff --check` | Passed |
| `cargo test --workspace --all-targets --all-features --no-fail-fast` | 965 passed, 1 failed, 19 ignored; **not a passing gate** |
| `node --test tests/pi_reporting_extension.mjs` | 18 passed |

The sole remaining failure is
`tests/server_lifecycle.rs:1773`,
`backpressured_input_and_send_do_not_block_inspect_or_kill`:
“input response unexpectedly completed while PTY stdin was backpressured”.
The exact focused command also fails on unchanged base `e241f50` in an isolated
checkout. No assertion was weakened or skipped. Investigating the unexpected
socket frame remains necessary before claiming the full regression gate passes.

New public-path tests in `tests/retained_sessions.rs` cover successful removal,
project removal, offline/restart archive retention, missing-directory reopening,
Dashboard hierarchy visibility, live/uncertain guards, dirty/locked Git refusal,
SQLite write failure and deferred commit failure, and provider-history sentinels.
A retained-store unit test covers atomic rollback and fencing old run callbacks.
Automated TUI tests cover confirmation/cancel, blocked/partial-failure notices,
path search/display, and unarchive without launching.

Regression checks failed before their fixes: stopped sessions blocked removal;
unarchived orphan rows disappeared from hierarchy; archive search omitted paths;
Git deleted an ignored recorded provider-history file. See the `red`,
`orphan-red`, `search-red`, and `history-red2` logs. The compile/fixture mistakes
in earlier logs are not counted as behavioral regression evidence.

## Native acceptance recipe for #131

Use [the existing disposable GUI workflow](../testing-computer-use.md), including
its permission, screenshots/accessibility, process tracking and cleanup rules.
Launch from the pinned checkout with `rtk proxy just gui`. No native provider is
required for the shell-based removal acceptance; native provider continuity and
Linux evidence remain separate, unverified gates.

1. Inspect the fixture's `OVRCR_CONFIG` and `OVRCR_SOCKET`; both must point into
   this launch's disposable root. Use only this checkout's `target/debug/ovrcr`.
2. Select a live fixture workspace. Use `Space w x`, accept the picker, inspect
   the archive warning, cancel once, then confirm. Verify live work blocks removal.
3. Stop every terminal in that workspace through the fixture CLI (`terminal kill
   ID`), retaining active stopped records. Verify recorded owned processes stopped.
   Rename one row to a recognizable title. Add a disposable untracked file and
   confirm dirty-worktree refusal; remove only that file afterward.
4. Confirm clean removal in Dashboard. Verify the workspace vanishes from the
   active hierarchy. Open `:` → Archived sessions. Search by title, workspace and
   an original path component; capture the visible title and path.
5. Unarchive one row. Verify it is visible and stopped, without a process. Reopen
   it and verify the missing-directory failure. Do not acknowledge unknown
   processes or create a replacement directory to bypass this case.
6. Remove the other workspaces of a disposable project using the same process,
   then unregister the project. Verify its archived context remains searchable.
7. Capture a removal-failure notice. The deterministic commit-failure fixture is
   `workspace_removal_blocks_live_and_uncertain_rows_and_preserves_records_on_failures`;
   its deferred SQLite constraint provides a reproducible storage-failure recipe
   if the acceptance agent needs that case in the GUI.
8. Close the GUI and verify launcher cleanup as documented. Retain screenshots,
   accessibility evidence, exact SHA, tool versions and any failed cleanup state.

## Review and remaining gates

Standards self-review: no remaining code findings against the runtime, TUI and
testing guides. Spec self-review: fixed the ignored provider-history deletion
case; no remaining implementation findings. These reviews were sequential in
the implementation session because no sub-agent tool was available; they are
not independent review.

The merged revision passes the local full regression gate as recorded above.
The 20 ignored cases, independent review, native acceptance #131 and Linux
acceptance are not certified. Draft PR #135 is open; it has not been merged or
marked fully accepted.

# Focused acceptance #127–#131 — 2026-09-17

Status: partial; no issue certified or closed. No implementation changes.

Tested source: `41a1db2719467741db201b924a3a5f474328b505`, exported with git archive into `/private/tmp/ovrcr-acceptance-127-131`. Native app built from that export with `SHELL=/bin/sh just gui`. Original main checkout remains at `ed273fc760fe4dbd387d8f70d9d42951018f7fc6` with pre-existing unrelated changes.

PRs #132–#135 are merged. GitHub macOS checks passed on their heads; #134/#135 Linux jobs were skipped under the current CI configuration. This is macOS acceptance only.

## Finding

**P2: partial-removal diagnostic is clipped.** On the default GUI window, the confirmation dialog shows only three diagnostic lines and ends at `but removal metadata was not`. It does not show the rest of the commit-failure diagnosis. Reproduce using the deferred `commit_guard` foreign key and `reject_commit` trigger from `tests/retained_sessions.rs::workspace_removal_blocks_live_and_uncertain_rows_and_preserves_records_on_failures`, then confirm removal through Dashboard. Actual Git removal and metadata rollback are correct, but the visible explanation is incomplete. See [screenshot](131-11-partial-failure.jpg) and [AX](131-11-partial-failure.ax.txt). Fix recommendation: allow the complete diagnostic to wrap/scroll within the confirmation, then retest this same default-size case. No fix made.

Archive paths also clip to their common prefix at this window size; full-path search works and JSON preserves the full value. See `131-06-archive-path-search` evidence. Full original path display at this geometry remains a UX gap.

## #131 native cases

| Case | Result / evidence |
|---|---|
| Open confirmation and cancel | PASS: `131-01-confirm`, `131-02-cancel`; shell printed distinct `ACCEPT_CANCEL_OK` after cancel. |
| Live sessions block removal | PASS: `131-03-live-refusal`; actionable SessionsRemain. |
| Uncertain ownership blocks removal | PASS: `131-10-uncertain-refusal`; an owned direct-exit shell left an uncertain row while other workspace sessions were stopped. |
| Dirty worktree blocks removal | PASS: `131-04-dirty-refusal`; owned dirty file preserved until explicitly removed by fixture setup. |
| Root protection | PARTIAL: root-named fixture workspace visible in hierarchy but excluded from removal picker (`131-12-root-excluded`); Enter reports unavailable choice. This was a root-named worktree, not the repository root itself; actual repository-root deletion guard not natively exercised. |
| Successful removal archives stopped records | PASS: auth-handoff and worktree-lifecycle removed through Dashboard; original titles/workspace preserved in archive. |
| Original path search | PASS: exact original workspace path matched archive rows (`131-06`). Path text is clipped visibly; JSON preserves full path. |
| Remove project after workspaces | PASS: consigint removed from project registry; archived context still found (`131-09`, `131-project-removed-archive.json`). |
| Unarchive without launch | PASS: row 8 restored stopped, PID null. Archive/unarchive advances run fences; an initial inspection assertion incorrectly expected unchanged run/closed phase and was corrected to the actual stopped/no-process contract. |
| Reopen missing directory | PASS: explicit palette Reopen then confirmation gives NotFound with original directory (`131-08`); no alternate directory or process. `131-07` is the preceding confirmation, not the result. |
| Owned Git failure | PASS: Git worktree lock yields Conflict; directory and records retained (`131-05`, `131-after-git-failure.json`). |
| Partial Git/storage failure | DATA PASS / GUI FINDING: path removed, full active record list unchanged, workspace metadata retained (`131-before-partial.json`, `131-after-partial.json`). Diagnostic clipped as above. |
| Provider-history preservation | PASS with limited fixture: four external sentinel files survived removal, hashes in `131-sentinels.json`. Recorded ignored in-worktree provider history covered by passing integration test, not native GUI. |

User actions used CUA. CLI was used for disposable fixture setup, shell stops, identity and file/process checks; no paid provider was used. Demo provider labels describe shell fixtures and are not provider evidence.

## Automated evidence

`SHELL=/bin/sh cargo test --all-features --test retained_sessions workspace_removal -- --test-threads=1`: **3 passed, 0 failed, 0 ignored, 15 filtered**. See `131-focused-tests.log`. Includes retained archive/project context, live/uncertain and partial failures, and ignored provider-history protection. No complete regression rerun. No new automatic-recovery race run was performed in this pass.

## #127–#130 prerequisites and remaining cases

- #127 BLOCKED: installed Claude is 2.1.274, outside pinned adapter certification (2.1.267 / 2.1.268). Need a supported executable; did not change adapter, downgrade installed tools, or bypass admission.
- #128 installed Codex 0.153.0 matches; #129 Pi 0.85.1 matches; #130 OMP 18.2.2 matches.
- Asked for account/model selection and bounded spend or turn cap, as required by each acceptance ticket. No response received during this pass. Account-backed runs and credential isolation remain pending.
- Thus **all native provider cases remain unrun**: exact conversation/recall, repeated recovery, native A→B→A, reporting freshness, supported input requests, launch counts/PID continuity, automatic eligibility/negative cases, acknowledgment cancel/confirm, capacity, provider prerequisite failures, Retry/new-row behavior and close resurrection.
- Retain Codex unavailable reporting for the whole resumed invocation; Pi Ready Confirmed and OMP Ready Observed; no cross-provider inference. Missing Pi/OMP history must be removed before reopen, not concurrently during startup.

## Cleanup

GUI closed through CUA; launcher exited 0. All 11 recorded shell process groups absent; fixture root and socket absent (`131-cleanup.json`). Short-lived direct-exit shell #11 separately has exit 0/no PID, with command known not to spawn descendants (`131-owned-exit-proof.json`). Source export remains in `/private/tmp/ovrcr-acceptance-127-131` for resuming blocked provider work. Evidence copied here before closeout. No user/live OVRCR processes were stopped.

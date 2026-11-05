# OVRCR UX pass — continuation on 2026-11-05

Original review and implementation started in Claude Code session `f1ff5062-a3ad-4b43-9e8d-58e185aea4cc`, against main `bdd30e77`. Codex recovered the checkpoint at `f5d1aa70` without changing the live installation or sessions. Work remains local; publishing and main merge require authorization.

## Recovered scope

| Area | Recovered state | Continuation |
| --- | --- | --- |
| Forms, attention, header, commands, folder names, onboarding | Integrated and reviewed by Claude | Preserve; combined verification pending |
| Workspace/project/welcome cards | Merge interrupted after conflict resolution | Finished as `4a4f4eff`; narrow unread clipping corrected, compact attention counts in `6bba13a0` |
| New task / initial agent prompt | Implemented on `ux/new-task` | Independent review and prompt-retry correction underway |
| Alternate-screen Transcript | Partial implementation on `ux/transcript` | Completing runtime, protocol, Dashboard and real-path tests |
| OMP scrollback, copy padding, Ctrl-o, wheel | Implemented on `ux/omp-scrollback` | Reviewed, corrected in locked vt100 source, integrated as `3a8fafbc` |

## Verification to date

- Workspace cards: `ovrcr-tui` initially 395 + 10 passed; permission-enabled root TUI 368 passed; both clippy feature modes passed. Further card tests: unit 12 passed; rendered Dashboard 7 passed.
- The 40-column unread-row test failed on the recovered adaptation, then passed after allowing its title to shrink before state and elapsed time.
- Initial root TUI startup failure repeated alone. Captured stderr proved private socket bind `Operation not permitted`; permission-enabled rerun passed. Failed attempts remain in `/private/tmp/ovrcr-ux-resume/`. Failed owned fixtures were removed after their server PIDs were confirmed gone.
- Required combined revision gates and native CUA run remain pending. Native macOS, Linux, real-provider restoration and clipboard delivery must be reported separately.

Original screenshots and accessibility text are retained in [before/](before/). Coordination and lane reports remain in `.superpowers/sdd/2026-11-03-ux-pass/`.

## Follow-up checks

- Narrow project rows previously lost the Ready count after long Input wording. Regression failed on the prior tree and passes with the shared attention strip’s compact `? 1 · ✓ 1` presentation. Card tests: 13 units + 7 real Dashboard cases passed.
- OMP review found that rebuilding the parser lost saved cursor/attrs, origin mode and scroll-region state. The correction vendors the exact locked MIT `vt100 0.16.2` package and implements ED3 by clearing only its active grid’s saved lines and offset. No version or dependency addition. The preservation regression failed before and passes after; the server path also passes.
- OMP merge resolved two additive conflicts, preserving L3’s error/refusal tests and adding L10’s three-row wheel check. All 39 merged copy/History tests passed.
- The new Transcript server regression passes for synthetic Claude and Codex conversations: current Dashboard owner, stale runs and references, same-run history-path replacement, unchanged source bytes, and continued terminal acknowledgement. Six transcript parser tests pass. Final UI review is underway.

## 2026-11-05 integrated follow-up

The earlier in-progress notes above are retained as history. All ten lanes are now integrated locally at `78ce52fd62ddc4522e53fe23afd97d29e6fe60c4` (product source unchanged since `76e3228b`; later commits update acceptance tests and documentation). No push, PR or main merge.

- **L8 complete:** `e9e8af7b` integrates the reviewed task-to-agent path. Retry never resends Task, and blank-Task configured arguments survive recovery. Initial prompt reaches the actual compiled Dashboard/Server/PTY as one literal argument. Inherited reporting authority is cleared in Live command fixtures.
- **L9 complete:** `76e3228b` integrates read-only Claude/Codex Transcript, protocol 71, current-owner/run/reference checks around bounded file reads, modal input suppression, cancellation and sliding pagination. Independent review approved. Public Dashboard tests include actual mouse forwarding before modal suppression; production Server tests cover both provider formats and source immutability.
- **L10 correction:** the initial parser-rebuild workaround was replaced before integration because it lost cursor/attribute/origin/margin state. The exact locked MIT `vt100 0.16.2` source is vendored with a small ED3 patch clearing only saved lines/offset, provenance and a failing-before preservation regression. No package/version addition.
- **Native walkthrough complete:** [dated evidence and outcomes](after-2026-11-05/README.md). Real shell markers, Unicode paste, workspace validation/creation/path, task derivation, cards, settings search, split chooser, Ctrl-O, shell History/ED3 and synthetic Transcript input safety were observed. The first fixture lost native control; the fresh retry completed the checks. Both cleanup receipts passed. Native-only gaps remain explicit in that record.

### Combined automated gates

The first whole-workspace run at `76e3228b` executed 53 test binaries: **2,591 passed, 8 failed, 32 ignored**, exit 101. Failures were GUI (7) and terminal acceptance (1). GUI assertions were adapted to intentional compact-header/quota-details behavior and an explicit palette-dismissal wait; startup limits were preserved. Copy/reattach now acknowledges the one-time status-off notice with an unbound F12 before asserting mode, preserving output/copy assertions. Product behavior and timeouts were not changed to obtain green results.

Corrected target runs: **GUI 10/10**, **terminal acceptance 23 passed, 1 ignored**, exit 0. Both integrated clippy modes passed with warnings denied; formatting and whitespace checks passed. A fresh serial whole-workspace run on `78ce52fd` is in progress; final result will be appended below. Failed attempts remain available in `/private/tmp/ovrcr-ux-resume/` and will not be relabeled passed.

### Remaining limits

Transcript intentionally skips individual JSONL records over 512 KiB, shortens prose after 16,384 characters and tool summaries after 160, omits tool results, and retains at most 1 MiB/16,384 entries in its sliding view. The local vt100 patch needs maintenance when that dependency is upgraded. Protocol 71 requires matching rebuilt server/client. Linux, hosted CI, real-provider restoration and clipboard delivery were not proven by this local pass. Publication remains outside this authorization.

### Final whole-workspace result — 2026-11-05

Fresh serial run on `78ce52fd`: **2,599 passed, 0 failed, 32 ignored**, across 53 test binaries; exit 0. [Machine-readable result](verification-2026-11-05/final-result.json), [full log](verification-2026-11-05/integrated-workspace-final.log), [all saved attempts](verification-2026-11-05/attempts.json). This supersedes the earlier in-progress status, while preserving the failed first run. Ignored checks are not passes; explicit installed-provider/capacity/memory gates remain unrun.

The native walkthrough and both cleanup receipts are complete. The final evidence commit changes only this research record, not the tested product or acceptance source. Original untracked `before/` captures and `.superpowers/` recovery state are preserved separately. No publication/main merge performed.

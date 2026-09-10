# Claude reporting implementation checkpoint

Worktree: `.worktrees/claude-reporting`, branch `codex/claude-reporting`, base `188a6456e42607634aedc72d3a93697faebcb0e1`.

Task 1 in progress. Existing Claude tests pass (3/3). Source fixtures are being captured separately from native evidence. Isolated CLI reached login selection after network permission was granted. No live turn has run. Authentication choice requested from user; no credentials copied or live settings edited.

Tasks 2–7 have not started: the plan requires source certification before dependent implementation. Required unresolved evidence includes foreground transitions, post-Stop settling and transcript/auxiliary accounting.

## Authenticated follow-up

User authorized existing subscription login. Attempt 05 captured native turns and approval behavior; both launches exited normally and the copied credential file was removed. See attempt-05-summary.md. Authentication is resolved. Task 1 remains open on provider-contract certification, including a newly observed distinction between SessionEnd(clear) and native process closing.

## Task 2 start decision

Independent gate review: Task1 permits concrete sources OR named blockers. Provider-independent Task2 may proceed using normalized synthetic reports without certifying Claude behavior. Ruling: start shared protocol/runtime ownership work now; keep native admission, confirmed settling, complete accounting and final-source completion as explicit provider/release gates. No unsupported capability is inferred from unverified cases.

Task2 implementer owns protocol/runtime source and tests; coordinator owns evidence/checkpoint. Native attempt07 adds compaction and foreground branch observations, while background fork was refused under unchanged fixture restrictions. Source certification checkboxes remain open.

Ruling: supervisor operation IDs are unguessable and private; retain current-epoch reservation receipt and latest mutating receipt, including finalization/release until next reservation. Historical operations after newer mutation return stale/conflict. Reservation status requires the original operation identity as well as authorization, not PTY capability alone. This bounds retained retry state; callers cannot retry arbitrary obsolete operations after later mutations.

Ruling: component source_sequence is optional but required for SourceIdentified, alongside identity. Lower sequence is rejected even under newer aggregate report revision; equality requires identical payload and preserves age. Uncertain sources cannot fabricate source order from arrival or reset the last identified watermark. This refines the shared schema without claiming Claude supplies such ordering.

Ruling: Task2 rejects decreasing known cumulative totals within the same binding/scope. No verified provider correction route exists yet, so no speculative correction flag is added. Unknown snapshots must not erase comparison watermarks. A future certified correction source needs its own explicit contract before such decreases are accepted.

Ruling: ReserveAgent is the first request on a persistent watched supervisor connection. Allocation installs its connection token atomically, so loss before SupervisorHello releases the reservation. A matching retry may reattach an active reservation; a released receipt never allocates again. Late cleanup must match the current connection token. A fresh explicit operation may reserve at the retained epoch. No orphan timeout is used.

## Next implementation boundary

Independent contract review permits Task3 launcher mechanics after Task2 review: exact native argv/stdio, watched reservation before spawn, lease isolation, process supervision and unbound release. Admission stays disabled; no transcript reads or provider metrics are authorized by this partial unit. An expected supervisor-selected session ID alone is insufficient root-origin proof because child events share the root ID. Enabling initial fresh-start admission still needs pinned conflicting-argv behavior, an attributable startup capture with complete field inventory, verified root/child discrimination, and one-shot conditional binding. Resume, fork and in-process switching remain separate gates.

## Task2 reviewed checkpoint

Task2 completed in `17548373cc210d3c7ebc4e3a6e2a9efcb4b0a2a5`. Independent reviewer verified that exact commit against `6d273eb7b6c035e48f4128092ad5977c71f6b587`, traced socket/dispatcher paths and assertions, and found no blocking issues. No additional reviewer tests ran. Final protocol22/runtime90, affected Clippy, workspace compilation, fmt and diff gates passed; full workspace regression remains incomplete. See task2/results.md and task2/review.md. Task1 certification remains open. Proceed with Task3 partial launcher mechanics under the boundary above.

## Task3 mechanics and fresh-start evidence

Task3 mechanics began after Task2 review. Child-owned process group with foreground handoff/restoration is approved; existing runtime pause/termination already signals attached terminal groups. Tests must exercise those real runtime paths. Missing reporting environment/server runs the native command with one reporting-unavailable diagnostic and no server autostart; explicit ownership conflict prevents spawn. Native argv is unchanged in this partial unit. It exposes no provider binding, transcript reads or metrics.

Attempt08 captured a named-root explicit-UUID startup and background child events with full field-name inventories. Independent source/evidence review permits the narrow initial startup discriminator pending real invocation routing tests. It does not certify foreground-child tool hooks, background conversation forks, in-process switches, confirmed settling or full accounting. Native PID/PGID94134 exited and were verified absent. See attempt-08-summary.md and initial-admission-contract.md.

## Task3 partial mechanics reviewed

Mechanics commit `32336e7` and correction `590b5d2` were independently reviewed. The only blocker (private reporting-channel setup aborting native execution) is resolved; fallback releases the lease before native spawn and strips all inherited reporting variables. Saved gates include 24 CLI, 3 report and 90 runtime tests, two original lifecycle tests, then three targeted CLI and three lifecycle correction tests; affected compilation/lint/fmt passed. See task3/results.md, p1-channel-failopen.md and review.md. Provider admission, collector and full Task3 acceptance remain open.

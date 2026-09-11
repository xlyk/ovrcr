# Independent assembled feature review

Reviewed committed implementation `b35dd71f5157b65afa47d55f222ffa4ea13a4cd0..2db1897409bead1e1221c9f508575cabe6564844` against `plans/2026-09-10-codex-response-ready-hooks.md`. The older investigation in the PR based at `983dd42` remains deliberately retained evidence for the superseded broader scope. Working-tree plan progress edits and the concurrent `src/report/admission.rs` fixture correction are outside this committed review.

## Finding — changes requested

**[P2] Preserve unavailable health in background Ready rows.** `crates/ovrcr-tui/src/dashboard/render.rs:1101-1106` appends unavailable after the full response-ready/quality text. The sidebar is capped at 40 columns (`:68`, `:882`) and its session row clips that suffix (`:1020`, `:1057`). Consequently a background Codex session that previously reached Ready and then loses its reporter displays the same Ready row as a connected reporter; there is no visible unavailable indication. The existing test explicitly expects the analogous clipped Claude row while unavailable (`crates/ovrcr-tui/src/dashboard/tests.rs:1314-1327`), and the metadata assertion only proves that the selected session can show health in a wide terminal. Single-pane metadata also appends health after PID, elapsed time, Ready and quality, so moderate widths hide health there too. This violates the Task 1 requirement to keep reporter health independently visible and is especially relevant to the intended background-readiness workflow. Prioritize unavailable health in the sidebar and single-pane metadata, as the split metadata path already does; add assertions against the actual background row and moderate-width metadata cells, not a whole-screen substring that a selected pane can satisfy.

Retaining the last activity sample itself is consistent with the existing runtime snapshot model: `ReportingState::unavailable/lost` retains activity and changes health. The correction does not require erasing observations or adding new runtime state. Clarify the plan/setup wording that reporting invalidation ends current validity but may leave a retained observation alongside visible unavailable health.

## Review coverage

Independently read the changed implementation, meaningful test assertions, affected callers, and architecture/runtime/TUI/testing/delivery guides. Traced protocol encoding/versioning and CLI activity serialization; managed launcher provider selection and native fallback; macOS/Linux peer credentials, parent relationship and unreaped-native lifetime anchor; private transport bounds; root event rejection/deduplication/closed-turn correlation; bounded identity storage; compare-and-exchange Bind and receipt recovery before publication; reporter failure and native completion; setup composition, quoting, idempotence, preserved handler order, version-only doctor and native no-op output. No further actionable implementation findings identified in those paths.

Inspected retained native Task 2 evidence as narrow evidence, not as proof of final acceptance. No tests or native GUI sessions were launched by this reviewer; the coordinator owns the full-suite run and final assembled acceptance. No source edits, staging or commits performed.

## Remaining acceptance gates

- Resolve and independently review the rendering finding and concurrent admission test-fixture correction on exact committed revisions.
- Complete workspace regression checks; retained attempts 1 and 2 failed startup fixtures and remain failures, not passes.
- Complete final reviewed-checkout native GUI acceptance: real child isolation, backtrack/new prompt, dashboard reconnect, second Ready and interruption; retain screenshots/accessibility, actual output, ownership and cleanup evidence.
- Record macOS native and Linux/platform results separately; current-head CI and required hosted capacity gates remain open until actual passing evidence.
- Update accepted support documentation/checkpoints and diary, then push the final revision to the separate PR. No merge or release approval is implied by this review.

## Scoped fixture correction review

Also reviewed committed `b661988361958b07131ea9e2e0bd76a0ec780666`. Approved: this changes only the descendant-cleanup fixture from a freshly written executable to `/bin/sh -c`, passing identity/version/exit as positional arguments. It still runs the same production `probe_command` and `classify_version`, retains every classification and process-cleanup assertion, and leaves the production probe budget and behavior unchanged. Focused execution evidence and the full regression result remain the coordinator's gates. The rendering finding above is still pending its separate correction.

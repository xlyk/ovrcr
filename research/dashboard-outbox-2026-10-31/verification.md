# Dashboard Outbox verification

Reviewed product source: **`4cb02011febebf7b2da46620d80acd309a294ffa`**, against base `07b792fb9854e38db671ec51fc81ecee25d70907`, for [specification #383](https://github.com/xlyk/ovrcr/issues/383) and [implementation #384](https://github.com/xlyk/ovrcr/issues/384). The separate test-observation correction is **`8a58800084ca32b9e98c083a52829e0a74c26184`**; product source remains unchanged. The [native record](README.md) identifies the matching frozen owning-crate patch and tested bundle.

**Both local Rust CI-profile gates completed with exit 0.** macOS includes two flaky cases; Linux passed without retries. Earlier Cargo failures remain failed and are preserved below. The [gate manifest](gate-manifest.json) records revisions, commands, counts and qualification. Compressed receipts are alongside this report; original logs remain in `/private/tmp/ovrcr-384-evidence/`.

## Observed results

| Check | Observed result and receipt |
| --- | --- |
| Original owning TUI baseline | 341 passed / 0 failed; log `01-baseline-tui.log`. |
| Original-code ownership RED | 1 failed / 2 passed; [02-red-outbox-dispatch.log](02-red-outbox-dispatch.log.gz). Public palette Inspect was absent from Outbox (0 vs expected 1); both original socket-order characterizations passed. |
| macOS owning TUI after migration | 345 library + 10 public integration passed; `09-green-tui.log`. |
| Linux owning TUI after migration | 321 library + 10 public integration passed; `10-linux-tui.log`. This is not a full Linux workspace result. |
| macOS root public Dashboard tests | 330 passed / 0 failed; `14-root-tui.log`, repeated within log 18. |
| Workspace typecheck / formatting | Exit 0; `15-workspace-check.log` and `17-fmt.log`; diff check clean. |
| Original full macOS gate, four test threads | **Exit 101: 2429 passed / 3 failed / 32 ignored**, across 53 test binaries; [18-macos-workspace.log](18-macos-workspace.log.gz). Failed: one copied-Server socket-readiness case and two GUI pre-header readiness cases. |
| Post-refactor real Server/outer-PTY application path | Included in log 18: `terminal_acceptance` 19 passed / 0 failed / 1 existing ignored fixture. New retarget case passed: old raw child receives exactly one captured release, replacement accepts fresh input, next old-child checkpoint is empty; cleanup assertions complete. |
| Isolated rerun of both failed macOS targets, one test thread | Exit 0: startup 20 passed / 0 failed / 2 ignored, GUI 10 passed / 0 failed; [32-macos-startup-gui-serial.log](32-macos-startup-gui-serial.log.gz). Assertions and deadlines unchanged. This serial result does not erase the original concurrent failures or establish their cause. |
| Clippy / formatting after test corrections | macOS Clippy exit 0 ([46-macos-fixture-clippy.log](46-macos-fixture-clippy.log.gz)), Linux Clippy exit 0 ([47-linux-clippy.log](47-linux-clippy.log.gz)), rustfmt exit 0 ([48-final-fmt.log](48-final-fmt.log.gz)); diff check clean. |
| Corrected Linux Pi prerequisite probe | `37-linux-node-ready.log`: Node v22.23.4, UID 1000, `registerHooks` is a function. Selected previously failing assignment-control case: 1 passed / 0 failed, 188 filtered in its owning binary; `38-linux-node-pi-case.log`. Zero-test filtered binaries add no coverage. |
| Corrected file-content and early-quota observations | Each exact case passed once on each platform: [42-macos-fixture-input.log](42-macos-fixture-input.log.gz), [43-linux-fixture-input.log](43-linux-fixture-input.log.gz), [44-macos-fixture-quota.log](44-macos-fixture-quota.log.gz), [45-linux-fixture-quota.log](45-linux-fixture-quota.log.gz). Other filtered binaries add no coverage. |
| Linux full run before final runner corrections | Exit 101: 2173 passed / 10 failed / 30 ignored across 52 binaries; [50-linux-workspace-final.log](50-linux-workspace-final.log.gz). Seven failures reported missing `SHELL`, one could not bind a relative socket in the read-only checkout. A preamble-read timeout and an early terminal-process abort were also recorded. |
| Second full macOS Cargo run | Exit 101: 2431 passed / 1 failed / 32 ignored across 53 binaries; [49-macos-workspace-final.log](49-macos-workspace-final.log.gz). The unchanged runtime listing-failure test returned `Conflict` where it asserted `OwnershipUncertain`. This run remains failed. |
| Corrected Linux runner checks | Log 52 verifies UID 1000, init, Node 22.23.4, Rust 1.98, `SHELL=/bin/zsh`, and actual relative-socket creation in the private source copy. Exact protocol, History-copy, WIP and relative-socket cases each pass once in logs 53–56; assertions and deadlines unchanged. The timeout and terminal abort did not reproduce in isolation; their original causes remain unproved. |
| Corrected Linux full Cargo run | Exit 101: 2182 passed / 1 failed / 30 ignored across 52 binaries; [57-linux-workspace-private-final.log](57-linux-workspace-private-final.log.gz). Copy-mode acceptance aborted at its first child startup, before Dashboard readiness. The new Outbox real-child case and the partial-resize case both passed. |
| Linux workspace doc tests | Exit 0, five crates, zero examples; [61-linux-doc.log](61-linux-doc.log.gz). This adds no executed-case coverage. |
| macOS CI-profile gate | Exit 0: **2431 passed, including 2 flaky cases; 33 skipped**, across 53 binaries, 2098.577 seconds; [60-macos-nextest-ci.log](60-macos-nextest-ci.log.gz). Three failed attempts occurred before the two retry passes. |
| macOS workspace doc tests | Exit 0, five crates, zero examples; [65-macos-doc.log](65-macos-doc.log.gz). This adds no executed-case coverage. |
| Linux CI-profile gate | Exit 0: **2182 passed, no retries; 31 skipped**, across 52 binaries, 1016.724 seconds; [64-linux-nextest-ci.log](64-linux-nextest-ci.log.gz). |

Ignored fixture entrypoints and skipped platform cases are not passing product cases. Linux runs use default features because its headless target lacks the macOS GUI backend; macOS full checks use all features.

The [actual CI workflow](../../.github/workflows/ci.yml) uses Nextest with the unchanged `ci` profile: separate test processes, two configured retries and its existing filter excluding `partial_resize_connection_delivers_error_before_owner_close`. That filtered case passed separately under Cargo on both platforms in logs 49/50/57. It is not counted as a Nextest pass. The macOS CI-profile result includes the retries below; it is not clean first-pass evidence. macOS used two test threads; Linux used four.

Log 60 records two flaky cases. `doctor_interrupt_cleans_its_owned_version_probe_before_exit` failed its five-second probe-readiness assertion on TRY 1 and passed TRY 2. `pause_resume_dashboard_round_trip` timed out before Dashboard readiness on TRY 1 and TRY 2, then passed TRY 3. That is three failed attempts across two cases. The two failed pause/resume attempts' logged Server PIDs and private roots/sockets are absent ([receipt](macos-ci-attempt-cleanup.json)); unrecorded process-group identity is not inferred from a PID.

Nextest 0.9.150 was obtained from its [official pre-built distribution](https://nexte.st/docs/installation/pre-built-binaries/) into task-only storage. Downloaded archive SHA256 values matched GitHub release-asset digests; no project dependency or global tool installation changed.

Linux log 62 exited 102 during Nextest metadata, before running any tests: its all-features metadata graph needed additional locked Linux GUI crates. Host `cargo metadata --locked` fetched them into the task-only cache (log 63, exit 0). The container stayed in network `none` mode; the attempted bridge connection was rejected without changing it. Log 64 ran offline with the original profile, filter and assertions.

## What the assertions prove

The new public-drain test requires exactly one Inspect and an empty second drain. Real dispatcher/socket assertions preserve captured cleanup → due HistoryPage → former action request, and queued SetView → newly scheduled HistoryPage. Deferred palette polling separately retains its original submission-before-History order. They check payloads, IDs, captured session/run/control epoch/view revision and no replay; ignored responses retain their no-drain boundary.

Existing History releases, cursor failure/retry, pending suppression, navigation/assignment input fences, clipboard ordering/failure selection, Operator/terminal independence, startup interleaving and view-before-remembered-save assertions remain. Repository callers consume the public outgoing drain; no replacement request-valued action was introduced. **No pre-existing request-order defect correction is claimed**: the original-code RED demonstrates the selected ownership contract, while original ordering characterizations already passed.

[Native screenshots and accessibility receipts](README.md) independently show original-session input, frozen loaded History, replacement-session input and return to the same original PID. Normal close, original launcher exit 0 and all 12 owned process-group/root/socket cleanup checks are retained. Native held-mouse retarget input, clipboard delivery and live providers were not exercised; exact captured release/order has separate socket and actual-child evidence. No paid evaluation or live provider call supplies validation here.

## Linux attempts that could not qualify

The initial full attempts exposed runner prerequisites rather than a completed workspace verdict:

- Parallel linkers were killed with signal 9 (log 16); serialized linking completed with one Cargo build job. No kernel OOM receipt was captured.
- A non-reaping `sleep` PID 1 left orphan zombies visible to the process-group assertion (log 19 and `linux-init-failure.json`); the corrected container uses init.
- Root bypassed mode-000 permission behavior (log 27); the exact assertion passed under UID 1000 in log 29.
- Node 22 had been omitted: an actual same-user invocation of the shared Pi host exited 127 with `exec: node: not found` (log 33). Log 30 was stopped before failure bodies printed; missing Node is not asserted as the proven cause of every failed name. The corrected Node/API probe and selected case above passed.

The later full run also exposed an unset `SHELL` and a read-only package directory. The corrected runner used a private writable copy of committed `8a588000` at the same `/work` path, with source-file mtimes preserved for the existing build cache. No host source was mounted. It contained the installed Rust/Clippy/zsh/Node tools and ran without network egress.

These corrections changed task-owned runner setup only. No product assertions, deadlines or runtime behavior were relaxed. The corrected Linux checks ran offline with UID 1000, init, `SHELL=/bin/zsh` and Node 22, with one Cargo build job. Cargo log 57 used two test threads; CI-profile log 64 used four.

The corrected Cargo run still recorded one startup abort. The exact `output.write(&bytes).is_ok()` expression belongs to Rust 1.98's fork-child setup/exec-error reporting channel. It establishes a child setup/exec error followed by a failure to report that errno; it does not identify Dashboard rendering output. Neither errno, the emitting stack nor the exact spawn instance was captured. The test failed before Dashboard readiness; whether the intended executable's main function ran is unproved. See the [bounded source audit](linux-runtime-abort-audit.md). The subsequent Linux CI-profile run used four test threads and separate test processes.

## Demonstrated test-observation races

Linux attempt 39 exposed two fixture races. The lifecycle-input case observed a marker after shell redirection created it but before `printf` wrote `delivered`. The quota case rejected a legitimate `QuotaChanged` event before its Hierarchy greeting, even though the event already contained the expected missing-Claude diagnostic.

Commit `8a588000` waits for exact completed marker contents, retaining the original assertion and adding an exact paste assertion. The no-Claude test retains early quota snapshots through a correlated Hello response, preserving its existing reason/redaction checks and deadlines. Shared empty-file barriers and the shared attach helper retain their semantics. Product code is unchanged.

Runs 36 and 39 were cancelled before qualification so the final checks could compile these corrections. Their partial results are not full passes. Owned macOS groups were stopped and verified absent (`40-macos-cancelled-gate.json`); only test processes in the dedicated Linux container were stopped (`41-linux-fixture-reset.log`).

Cancelled log 39 also contains a third failing name, `client_with_wrong_protocol_version_is_refused`, whose binary was interrupted before printing its failure body. The two diagnosed fixture failures do not exhaust that attempt's failing names. Its unknown cause is not inferred from the later log-50 preamble timeout.

## Independent review

Both independent reviews cover base → `4cb02011`: Standards **PASS, 0 blocking findings**; Spec **PASS, no findings**. The only Standards FYI is P3 naming: `enqueue_request` versus `push_request` may obscure their documented History ordering distinction. This optional naming suggestion remains unchanged. Full reports: [Standards](review-standards.md), [Spec](review-spec.md). Test results are established by the separate runner receipts above.

Scoped Standards and Spec re-reviews of `4cb02011..8a588000` also pass, with both observed fixture failures addressed and no new findings. Reports: `review-fixtures-standards.md` and `review-fixtures-spec.md`.

The [evidence consistency review](review-evidence.md) found two omissions: the already-observed Nextest retry and the third failure marker in cancelled log 39. Both are recorded above. This was a documentation check; it did not rerun or certify pending tests.

## Resource limits and delivery

Native cleanup has direct process-group, root, socket and original-launcher evidence in the [native record](README.md). The two failed Nextest pause/resume attempts also have logged Server PID/root absence receipts. This does not certify unrecorded groups from other failed attempts: the runtime panic and first doctor-readiness failure skipped paths that can leave children. A bounded host attribution pass found no readable exact matching helper; 285 argument reads returned EINVAL, so universal absence remains unproved. No unknown process was signaled. The [cleanup audit](failed-attempt-cleanup-audit.md) preserves the source and observation limits.

The disposable Linux container and custom runtime image were removed after the original test waits completed ([cleanup receipt](linux-runner-cleanup.json)). Only the task target and isolated Cargo cache had ownership restored to host UID/GID 501:20; the global registry was mounted read-only. The source archive, offline caches and task-only Nextest binaries remain as recovery state. The primary checkout and branch remain for review. No push, pull request, hosted CI or merge has been performed for this implementation.

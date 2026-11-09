reviewer_model: openai-codex/gpt-6.1-sol

# Cost mechanisms in the macOS checks tests

## Ranked mechanism exposure across the whole inventory

| Rank | Mechanism class | Rows with a mechanism signal | Timed top-level rows | Associated measured seconds | What the number means |
|---|---|---:|---:|---:|---|
| 1 | Process/PTY spawn and teardown | 1,060 | 878 | 4,422.670 | Tests tagged with real processes, PTYs, servers, provider hosts or bridge fixtures. Repeated setup and CLI spawning are the first optimization target. |
| 2 | Poll loop | 320 | 320 | 2,378.456 | Tests tagged with polling, timed loops or settles. This includes work between polls; it is not time spent sleeping. |
| 3 | SQLite/fsync durability | 228 | 228 | 1,906.336 | Tests tagged with SQLite, DB triggers, durable storage or registry locks. Keep durable commits and fault injection; remove surrounding fixture work instead. |
| 4 | Git/network-ish work | 180 | 180 | 1,010.919 | Tests tagged with Git, worktrees, remotes, HTTP, npm or network work. Cloud tests use a local bridge, not the Cursor service. |
| 5 | Fixed sleep candidates | 40 | 40 | 331.724 | Explicit settles, quiet windows and fixed-delay signals, excluding ordinary polling sleeps and long-lived child commands. Some are essential negative observation windows. |
| 6 | Deliberate timeout/deadline budget | 84 | 83 | 324.486 | Tests tagged with deadlines, timeouts, quiet windows or wall-clock budgets. Most watchdog limits are ceilings, not delays consumed on success. |
| 7 | Serialization: actual nextest exclusive scheduling | 4 | 4 | 37.240 | The four exact tests in `.config/nextest.toml:15-22`. Their reservation blocks other tests. This is not the process-local `env_lock`. |
| — | Pure compute | Unresolved | Unresolved | Unattributed | 56 hash/compute-tagged rows account for 471.095 s, but hashing tags include subprocesses. The evidence does not establish a pure-compute bottleneck. |

These are overlapping **exposure totals**, ranked by associated test duration. They are neither a decomposition of runtime nor removable seconds. A PTY test can appear in the first four rows. Adding this table double-counts tests. The source-backed examples below establish mechanisms, but the hosted log has no phase timings, CPU samples, fsync timings or wait-stack attribution. Therefore each “dominant” classification below is a source-based inference unless a test must exhaust a named real-time budget.

Accounting source: `research/macos-test-review/tests.csv` (3,171 rows; supplied SHA-256 `dcccdb0ec9d19a3569a05285f33f7620e71308f3765edf2d0086996ca767fb5a`) and `research/macos-test-review/evidence/attempts.csv`. All inventory rows were considered, including the 239 without individual timings. There are 2,932 timed rows; four are nested children whose times are already inside their parents. The timed top-level sum is 5,952.278 s, including the main nextest sum of 5,931.968 s. Unknown Python timings were not set to zero or divided out of suite time.

Reproduction rule for the table: parse `cost_drivers` as a JSON array; when the cell is plain prose, use that prose as one descriptor. Match case-insensitively, include a row at most once per class, and sum `total_attempt_seconds` only for rows with an empty `parent_test_id`. The literal patterns were:

```text
process: pty|process|subprocess|spawn|real server|binary server|dashboard.*host|node .*host|node fixture|scripted .*host|bridge fixture|server restart|real restart
poll: poll|\b\d+\s*(?:ms|s|second)[- ]*(?:answer )?loops?|settle
durability: sqlite|\bDB\b|database|fsync|durab|registry lock
git/network: \bgit\b|worktree|remote rewrite|network|\bHTTP\b|\bnpm\b
fixed: \breal sleeps\b|\d(?:\.\d)?\s*s?\s*settle|\d\s*ms settle|quiet window|settle sleep|sleep\(\d+ms\)|\d+ms sleep|version sleep|internal sleep|real backoff sleep|provider sleeps
deadline: deadline|timeout|wall.clock budget|quiet window|race window|60 s real|permission.*budget
hash/compute candidate: pure compute|CPU|computation|hashing|sha256
```

Scheduling uses the four exact configuration names, not descriptor matching. The excluded descriptor phrase `env_lock serialization` is misleading for process-per-test nextest execution: `tests/server_lifecycle.rs:32-46` is a process-local mutex. “JSON serialize/deserialize” is also not scheduler serialization. Mechanism tags are discovery aids, not proof. In particular, several cloud rows say “0.42 s observed” or “0.45 s observed”; those strings conflict with the matching attempt ledger and were ignored as timings. No duration in this report comes from those strings.

Source examples supporting the ranked classes: private Git/server/PTY construction in `tests/support/live.rs:111-145,239-257,341-389,653-684`; teardown in `tests/support/live.rs:548-564,581-629`; repeated cloud fixtures in `tests/cursor_cloud.rs:408-437`; 10 ms terminal polling in `tests/cursor_cloud.rs:125-135`; production CLI spawning and 100 ms readiness retries in `src/ovrcr-pi-runtime.ts:193-201,340-344`; durable-before-effect SQL in `crates/ovrcr-runtime/src/retained.rs:643-675`; a dependency transaction in `crates/ovrcr-runtime/src/server/delegation_dependencies.rs:173-180`; 3 s and 5 s title sleeps in `tests/automatic_titles.rs:763-783,2050-2062,2197-2221`; and real deadline assertions in `tests/claude_quota_probe.rs:360-389`.

## How to read the per-test savings

The next 25 entries are selected by summing `seconds` across attempts for each `(binary_id, test_name)` in `evidence/attempts.csv`, then sorting descending. Retry summaries are not additional attempts. The selected tests account for **974.758 process-seconds**, including the failed parent-recovery attempt. Only rank 6 retried in this selection. This is a single hosted run at revision `992a1da5c59a865ebb48051e4d9d8ff7184c8dc5`, run 42429838269/job 128576086691. No tests or fixtures were executed for this analysis.

There is no defensible nonzero expected saving from this log alone. Each range below starts at zero and gives a **conditional planning ceiling**, not a speedup forecast. For a matrix with N equally costly cases and one fresh server fixture per case, the gross repeated-setup envelope is `T × (N−1)/N`. It assumes all per-case time were setup, which overstates achievable savings because real case work remains. A phase trace must replace that envelope before a runtime target is promised. Socket/file notification ceilings instead use the actual polling period times the number of completed waits. Neither kind of range may be added to a pruning/consolidation saving on the same test.

For nonexclusive work, a removed process-time amount S has idealized wall equivalents of **S/3 on 3 occupied slots** and **S/4 on 4 occupied slots**. This is a slot model, not a claim that waits use a CPU core. The measured average was 5,931.968/2,014.918 = 2.944 concurrent processes; its matching quotient is S/2.944. At 3 cores this is about 98% of three slots; at 4 cores about 74% of four slots. Critical paths, blocked sockets, I/O contention and exclusive tests prevent a direct conversion to job savings. Exclusive reservations instead occupy wall time themselves and leave 2 or 3 slots unused; removing a reservation safely could unlock up to 74.480 or 111.720 slot-seconds across the four tests, not necessarily shorten the job by either amount (`.config/nextest.toml:15-22`). None of the top 25 is one of those four tests.

For every proposed change, collect separate elapsed spans for fixture construction, subject action, each wait, shutdown and destructor; count CLI/Git spawns and polling iterations. Then compare repeated isolated runs and full hosted runs at both scheduling capacities, checking all original assertions and resource-absence proofs. This is the measurement needed, not verification performed here.

### Shared helper chains

Cloud entries use `Fixture::new` (`tests/cursor_cloud.rs:21-85`), which creates a private Git repo, server, worktree and ordinary shell before launching a managed agent (`tests/cursor_cloud.rs:87-114`). Terminal reads use a fresh control connection and a 10 ms poll with a 15 s failure ceiling (`tests/cursor_cloud.rs:116-135`; `tests/support/live.rs:327-334`). Production resolves local Git identity, starts a Python bridge, polls its ready line every 10 ms, checks GetVersion before account/catalog calls, then uses bounded loopback RPC (`crates/ovrcr-runtime/src/cursor_cloud.rs:93-130,313-365,376-408,481-588`). The fixture uses `ThreadingHTTPServer` on loopback and explicitly never contacts Cursor (`tests/fixtures/cursor-cloud/bridge.py:1-2,273-286`). Do not call this real-network latency.

Delegation entries use `RecoveryFixture` (`tests/pi_delegation.rs:1511-1642`), including a serial fault proxy, a real server/worktree, a Node native-provider stand-in and a managed parent PTY. Parent readiness is polled every 10 ms until both binding and `PI_NATIVE_READY` appear. `tool_reply` sends actual terminal input, then polls a reply file every 10 ms with a 20 s failure ceiling (`tests/pi_delegation.rs:45-84`). The host runs the real registered handler (`tests/fixtures/pi/pi_host.mjs:215-228`). Production's extension starts a compiled CLI for each control call and may perform 100 SendInitial calls with a 100 ms pause (`src/ovrcr-pi-runtime.ts:193-201,340-344`). The proxy sets observation waits to zero so it does not block mutation ordering (`tests/pi_delegation.rs:1255-1261`). `OwnedFollowup` proves process groups, server and root are gone after teardown (`tests/pi_delegation/followups.rs:25-58`). Do not bypass this chain with direct DB seeding or substitute fake success responses.

A safe cloud matrix-sharing implementation would keep one server/repository but create a new managed session and bridge per case, record the event-log start offset, assert the original per-case event sequence, and close/discard/delete through the public protocol. Assert no live case process, payload or trigger remains before the next case. If those preconditions cannot be proved for a fault case, keep separate servers and share only an immutable Git seed. Never share fixtures between nextest processes or independent tests.

## Per-test teardown

### 1. `managed_cloud_refuses_incompatible_bridge_before_account_requests`

Suite: `ovrcr::cursor_cloud`. Measured: **68.639 s, 1 attempt**. Definition: `tests/cursor_cloud.rs:371-438`. Timing: `research/macos-test-review/evidence/attempts.csv`, matched on the suite and full test name above.

Test ID: `nextest|Test|ovrcr%3A%3Acursor_cloud|managed_cloud_refuses_incompatible_bridge_before_account_requests|default`.

Dominant candidate: **process/PTY spawn and teardown**. Thirteen incompatible version/capability responses each construct a whole `Fixture`, then launch one agent and wait for validation failure. The exact `["GetVersion"]` event assertion proves rejection before credentialed account calls (`tests/cursor_cloud.rs:371-438`). There is no fixed 60 s wait here. Production performs the capability checks at `crates/ovrcr-runtime/src/cursor_cloud.rs:563-588`.

Fix: hoist the repository/server fixture out of the 13-case loop; give every case a fresh session/bridge and a case-local log slice, with the reset proof described above. Keep all thirteen inputs and exact no-account/no-mutation assertions. Alternatively share only a private immutable Git seed if per-case cleanup cannot be guaranteed. Risk: stale bridge configuration or an earlier case's event could falsely satisfy the next case. The bridge reads its adjacent config on startup (`tests/fixtures/cursor-cloud/bridge.py:21-24`), so restart the bridge for every input.

Expected-saving planning range: **0–63.36 s process time**, the deliberately loose `68.639 × 12/13` setup envelope; ideal wall equivalents 0–21.12 s at 3 slots and 0–15.84 s at 4. Confirm by timing each construction/drop separately from each failed validation; retain zero credentialed calls in every case.

### 2. `claude_probe_startup_without_usage_reaches_the_deadline_and_the_process_is_gone`

Suite: `ovrcr::claude_quota_probe`. Measured: **62.920 s, 1 attempt**. Definition: `tests/claude_quota_probe.rs:356-358`. Timing: `research/macos-test-review/evidence/attempts.csv`, matched on the suite and full test name above.

Test ID: `nextest|Test|ovrcr%3A%3Aclaude_quota_probe|claude_probe_startup_without_usage_reaches_the_deadline_and_the_process_is_gone|default`.

Dominant mechanism: **deliberate timeout/deadline budget**, not slow computation. The helper waits for a quota event matching missing usage, asserts at least 60 s elapsed, and proves the probe PID is gone (`tests/claude_quota_probe.rs:356-389`). `start_probe` installs a fixture provider and starts the server (`tests/claude_quota_probe.rs:53-99`). Production uses `PROBE_LIMIT=60 s`, polls output/callback/process state every 20 ms, and kills/reaps the child on expiry (`crates/ovrcr-runtime/src/server/quota_probe.rs:18-18,317-375,384-394`). The test's 75 s socket limit is only a watchdog.

Fix: keep the real 60 s deadline. Profile and optimize fixture Git/server setup and post-expiry reaping only; use a socket-ready notification in place of 5 ms socket polling if that phase is material (`tests/support/live.rs:699-715`). Risk: shortening the real deadline or deleting the elapsed assertion loses protection against terminating cold startup after its early callback but before usage arrives.

Expected-saving planning range: **0–2.920 s process time**, the measured total minus the mandatory 60 s. That is at most 0.973/0.730 s ideal wall at 3/4 slots. Measure setup, expiry-event publication and teardown; no saving from the protected minute is included.

### 3. `claude_probe_silent_for_sixty_seconds_times_out_and_the_process_is_gone`

Suite: `ovrcr::claude_quota_probe`. Measured: **62.912 s, 1 attempt**. Definition: `tests/claude_quota_probe.rs:351-353`. Timing: `research/macos-test-review/evidence/attempts.csv`, matched on the suite and full test name above.

Test ID: `nextest|Test|ovrcr%3A%3Aclaude_quota_probe|claude_probe_silent_for_sixty_seconds_times_out_and_the_process_is_gone|default`.

Dominant mechanism: **deliberate timeout/deadline budget**. This is the distinct silent-provider path, rather than startup-without-usage. It shares the real 60 s and PID-absence helper (`tests/claude_quota_probe.rs:351-353,360-389`). Production distinguishes no callback at expiry from a callback lacking usage (`crates/ovrcr-runtime/src/server/quota_probe.rs:317-375`).

Fix: retain the silent mode, the 60 s minimum, Unavailable classification and real process cleanup. Optimize only setup and post-deadline overhead as in rank 2. Risk: combining these two tests into one provider mode loses the distinction between `PROBE_TIMED_OUT` and `PROBE_MISSING_USAGE`.

Expected-saving planning range: **0–2.912 s process time**, `62.912−60`; ideal wall 0–0.971/0.728 s at 3/4 slots. Confirm with phase timings and both original reason assertions. Do not count 75 s as spent time.

### 4. `managed_send_uncertainty_and_malformed_evidence_never_retry_or_become_ready`

Suite: `ovrcr::cursor_cloud`. Measured: **56.663 s, 1 attempt**. Definition: `tests/cursor_cloud.rs:1705-1793`. Timing: `research/macos-test-review/evidence/attempts.csv`, matched on the suite and full test name above.

Test ID: `nextest|Test|ovrcr%3A%3Acursor_cloud|managed_send_uncertainty_and_malformed_evidence_never_retry_or_become_ready|default`.

Dominant candidate: **process/PTY spawn and teardown** repeated across nine uncertainty/malformed-evidence cases, plus durable operations and terminal polling (`tests/cursor_cloud.rs:1705-1793`). Each case waits for preview, a retained handle, unknown observation and rejected resubmission. Production verifies source-tool correlation and terminal status rather than trusting a reply (`crates/ovrcr-runtime/src/cursor_cloud/run.rs:101-160,590-602`).

Fix: share the server/repository across the nine cases, but start a fresh bridge/session and use log deltas. Preserve exactly one Send and one CreateAgent, no ListRuns/ResumeAgent, no Ready, and the payload-deletion assertion after public discard. Risk: a shared agent/run or stale “unknown” terminal text would hide cross-run acceptance. Polling must be keyed to the new session, not to any terminal.

Expected-saving planning range: **0–50.37 s process time**, `56.663 × 8/9`; ideal wall 0–16.79/12.59 s at 3/4 slots. Construction and cleanup spans must establish the much smaller real saving.

### 5. `dependencies::public_dependency_lost_requests_and_replies_reconcile_original_baseline_without_duplicate_effects`

Suite: `ovrcr::pi_delegation`. Measured: **55.590 s, 1 attempt**. Definition: `tests/pi_delegation/dependencies.rs:1241-1316`. Timing: `research/macos-test-review/evidence/attempts.csv`, matched on the suite and full test name above.

Test ID: `nextest|Test|ovrcr%3A%3Api_delegation|dependencies%3A%3Apublic_dependency_lost_requests_and_replies_reconcile_original_baseline_without_duplicate_effects|default`.

Dominant candidates: **Git work plus process/PTY spawn and teardown** in a six-case transport-loss matrix. Each case creates a new owner, executes/accepts coding task A, then executes task B from A's authenticated baseline and replays original requests (`tests/pi_delegation/dependencies.rs:1241-1316`). The helper issues actual tool calls and waits for child publication (`tests/pi_delegation/dependencies.rs:11-49,98-100`). Production commits graph/member changes atomically and validates immutable coding provenance (`crates/ovrcr-runtime/src/server/delegation_dependencies.rs:148-180`; `crates/ovrcr-runtime/src/server/delegation_coding.rs:537-575`). The child performs dependency fixture work before publishing (`tests/fixtures/pi/pi_host.mjs:371-401`).

Fix: reuse a server/private base repository across cases while creating a fresh parent/workflow, uniquely suffixed task/workspace/branch IDs, and new A/B resources for every loss point. Keep baseline and candidate byte snapshots and both replay checks. Cache only immutable fixture files, never an accepted baseline or receipt. Risk: pre-accepting A in a reused workflow could mask dependency selection or authority defects.

Expected-saving planning range: **0–46.33 s process time**, `55.590 × 5/6` setup envelope; ideal wall 0–15.44/11.58 s at 3/4 slots. Count Git invocations and timestamp fixture, A, B, replay and teardown to separate necessary baseline work from repeated setup.

### 6. `parent_recovery::parent_recovery_transfer_discovers_prepared_and_suspends_accepted_unknown_partial_effects`

Suite: `ovrcr::pi_delegation`. Measured: **46.250 s, 2 attempts**. Definition: `tests/pi_delegation/recovery.rs:801-911`. Timing: `research/macos-test-review/evidence/attempts.csv`, matched on the suite and full test name above.

Test ID: `nextest|Test|ovrcr%3A%3Api_delegation|parent_recovery%3A%3Aparent_recovery_transfer_discovers_prepared_and_suspends_accepted_unknown_partial_effects|default`.

Dominant candidate: **process/PTY spawn and teardown**, amplified by one retry. The attempt ledger contains a failed 24.507 s attempt and a passing 21.743 s attempt, totaling 46.250 s. The body has four partial/prepared input scenarios; each starts an owner and a second managed parent, transfers authority, then proves the original receipts and byte counts stay unchanged (`tests/pi_delegation/recovery.rs:801-911`). The body does **not** restart the server, despite the broad CSV “server restart” descriptor. Second-parent readiness uses 20 ms polling (`tests/pi_delegation/recovery.rs:459-500`). Production's original-input handling retains the captured parent/control authority (`crates/ovrcr-runtime/src/server/delegation.rs:1691-1747`).

Fix: use one server/private base repo for four fresh independent workflows/parents, keeping separate fault rules and outputs. Replace binding/readiness polling with an acknowledgement for the exact new parent, while retaining the existing bounded failure and full byte/receipt checks. Risk: reusing the original parent/workflow would no longer exercise a true authority transfer. Retry cause is unknown; do not remove retries on this evidence.

Expected-saving planning range: **0–34.69 s process time** for the two observed attempts under the equal-case setup envelope (`46.250 × 3/4`). It includes no separately claimed retry elimination. Ideal wall 0–11.56/8.67 s at 3/4 slots. Measure each attempt and state-transition wait; inspect the failed attempt's actual assertion before attributing flakiness.

### 7. `managed_send_crash_and_discard_boundaries_preserve_uncertainty_and_exact_late_identity`

Suite: `ovrcr::cursor_cloud`. Measured: **46.006 s, 1 attempt**. Definition: `tests/cursor_cloud.rs:1940-2050`. Timing: `research/macos-test-review/evidence/attempts.csv`, matched on the suite and full test name above.

Test ID: `nextest|Test|ovrcr%3A%3Acursor_cloud|managed_send_crash_and_discard_boundaries_preserve_uncertainty_and_exact_late_identity|default`.

Dominant candidate: **process/PTY spawn and teardown** across eight cases: four Send boundaries crossed with crash versus discard (`tests/cursor_cloud.rs:1940-2050`). Crash here exits the adapter, not the whole OVRCR server. Checkpoint mode waits for the named marker before public discard and release. The production checkpoint has a 30 s failure ceiling and a 10 ms release poll; normal cases do not wait 30 s (`crates/ovrcr-runtime/src/cursor_cloud.rs:412-431`; `crates/ovrcr-runtime/src/cursor_cloud/run.rs:604-612`).

Fix: share the server/repository only. Keep a new bridge/session/checkpoint path per case, then publicly clean up each operation before advancing. Preserve the after-claim distinction (zero RPCs on that crash), null terminal state, exact late run ID and no Ready assertion. Risk: a generic ready handshake cannot replace the named boundary; it would miss claim-before-effect races.

Expected-saving planning range: **0–40.26 s process time**, `46.006 × 7/8`; ideal wall 0–13.42/10.06 s at 3/4 slots. Measure case setup versus claim/reply/ack waits; never reduce the checkpoint budget merely because it is large.

### 8. `followups::public_followup_storage_limit_preserves_last_result_acceptance_and_all_receipts`

Suite: `ovrcr::pi_delegation`. Measured: **42.056 s, 1 attempt**. Definition: `tests/pi_delegation/followups.rs:529-600`. Timing: `research/macos-test-review/evidence/attempts.csv`, matched on the suite and full test name above.

Test ID: `nextest|Test|ovrcr%3A%3Api_delegation|followups%3A%3Apublic_followup_storage_limit_preserves_last_result_acceptance_and_all_receipts|default`.

Dominant candidates: **SQLite/fsync durability and repeated process calls**, not a 10 s sleep. The same child completes and accepts 64 followups, retaining all history and receipts, then verifies exhaustion and exact final settlement (`tests/pi_delegation/followups.rs:529-600`). `complete`, `followup` and `accept` use real registered tools (`tests/pi_delegation/followups.rs:62-85`). Production's limit is 64 and it saves followup intent/results around effects (`crates/ovrcr-runtime/src/server/delegation_followups.rs:4-4,343-346,422-423,535-535,593-609`). The child's settlement gate polls at 10 ms until removed (`tests/fixtures/pi/pi_host.mjs:409-416`).

Fix: retain all 64 real followups. Add a fixture reply/publication-ready notification carrying the exact output path after its file is closed; await it instead of `wait_file` polling, then read and assert the same file. Notify exact final settlement too. This changes fixture waiting, not the production handler, provider process or durable writes. Risk: “published” precedes settlement; keep the held-settlement proof and binding/identity checks, rather than treating publication as Ready.

Expected-saving planning range: **0–2.56 s process time** from at most 256 completed 10 ms waits in the 64-iteration body (followup reply, publication file, collect reply, acceptance reply). This excludes initial/final waits and CPU reductions from fewer reads. Ideal wall 0–0.85/0.64 s at 3/4 slots. Count those waits and timestamp file-close to observation; if publication/SQL dominates, this saving will approach zero. Do not claim the entire 42.056 s removable.

### 9. `managed_creation_crashes_retain_original_operation_and_require_explicit_discard`

Suite: `ovrcr::cursor_cloud`. Measured: **39.866 s, 1 attempt**. Definition: `tests/cursor_cloud.rs:609-768`. Timing: `research/macos-test-review/evidence/attempts.csv`, matched on the suite and full test name above.

Test ID: `nextest|Test|ovrcr%3A%3Acursor_cloud|managed_creation_crashes_retain_original_operation_and_require_explicit_discard|default`.

Dominant candidate: **process spawn/restart and teardown**. Four creation-crash cases each construct a fixture and perform two real server restarts, proving uncertainty survives the first restart and deleted cloud state stays absent after the second (`tests/cursor_cloud.rs:609-768`). SQLite loads and durable operation files are part of the subject, not disposable setup. Production crash gates are explicit diagnostic boundaries (`crates/ovrcr-runtime/src/cursor_cloud.rs:412-431`); server launch/join is real (`tests/support/live.rs:239-257,492-532`).

Fix: hoist only repository/server-fixture construction; retain both restarts in every case, the same store across each restart, public close/discard/delete operations and event counts. Require an empty payload directory and no previous case's live session before the next case. Risk: replacing a restart with an in-memory reload loses persistence/replay coverage; copying a fresh database over the old one invalidates the test.

Expected-saving planning range: **0–29.90 s process time**, `39.866 × 3/4` gross setup envelope. The eight subject restarts remain, so actual saving must be well below that ceiling. Ideal wall ceiling 9.97/7.47 s at 3/4 slots. Measure initial fixture construction separately from each mandatory restart.

### 10. `managed_recovery_account_source_and_unknown_outcomes_fail_closed`

Suite: `ovrcr::cursor_cloud`. Measured: **37.103 s, 1 attempt**. Definition: `tests/cursor_cloud.rs:3351-3473`. Timing: `research/macos-test-review/evidence/attempts.csv`, matched on the suite and full test name above.

Test ID: `nextest|Test|ovrcr%3A%3Acursor_cloud|managed_recovery_account_source_and_unknown_outcomes_fail_closed|default`.

Dominant candidate: **process/PTY spawn and teardown plus local Git work**. Six modes establish an exact run, detach it, mutate account/credential/source/identity/status/missing evidence, and reopen through the public API (`tests/cursor_cloud.rs:3351-3473`). Production rereads source/account and credential binding before recovery (`crates/ovrcr-runtime/src/cursor_cloud.rs:481-499`); the source check runs local Git, while fixture publication avoids `ls-remote` (`crates/ovrcr-runtime/src/cursor_cloud.rs:93-130`).

Fix: share an immutable private Git seed across six separate `Live` roots, or reuse one server only if every account/config/remote and retained-state reset is proved. Keep a fresh initial exact run and reopen per mode. For source mode, keep the actual `git remote set-url`; it is the stimulus, not a redundant command. Risk: a cached profile/source result or resetting by direct SQL would bypass fail-closed recovery validation.

Expected-saving planning range: **0–30.92 s process time**, `37.103 × 5/6` gross repeated-setup envelope; ideal wall 0–10.31/7.73 s at 3/4 slots. Seed-copy versus real Git initialization must be timed before treating any portion as expected savings; preserve Git identity and remote isolation.

### 11. `desktop_notifications_bridge_expired_permission_check_recovers_without_reauthorization`

Suite: `ovrcr::server_lifecycle`. Measured: **35.784 s, 1 attempt**. Definition: `tests/server_lifecycle.rs:18457-18543`. Timing: `research/macos-test-review/evidence/attempts.csv`, matched on the suite and full test name above.

Test ID: `nextest|Test|ovrcr%3A%3Aserver_lifecycle|desktop_notifications_bridge_expired_permission_check_recovers_without_reauthorization|default`.

Dominant mechanism: **deliberate timeout/deadline budget**. This spends the production 30 s permission-poll budget, then another 1.2 s quiet window proving polling stopped. The 36 s test deadline is a watchdog (`tests/server_lifecycle.rs:18464-18509`). The real dashboard PTY/bridge fixture is built through `tests/server_lifecycle.rs:11895-11918,11935-11960`; detach/cleanup remains real (`tests/server_lifecycle.rs:12545-12581`). Production polls status roughly once per second under the 30 s budget (`crates/ovrcr-tui/src/dashboard/desktop.rs:49-49,541-566`).

Fix: keep the 30 s elapsed assertion and 1.2 s quiet window, which exceeds one production poll interval. Optimize only fixture setup/PTY teardown; do not shorten the permission wait or replace it with a fake clock in this acceptance test. Risk: shortening the quiet interval below one poll interval could pass even if automatic polling continues. Removing the real event-loop wait loses coverage of expiry without alerts/settings transitions and of explicit Status versus repeated authorization.

Expected-saving planning range: **0–4.584 s process time**, `35.784−30−1.2`; ideal wall 0–1.528/1.146 s at 3/4 slots. Measure setup and explicit recovery interactions. This test is not one of the four exclusive nextest cases.

### 12. `independent_review::public_review_partial_and_unknown_repairs_keep_original_receipt_without_replay`

Suite: `ovrcr::pi_delegation`. Measured: **35.158 s, 1 attempt**. Definition: `tests/pi_delegation/review.rs:1554-1633`. Timing: `research/macos-test-review/evidence/attempts.csv`, matched on the suite and full test name above.

Test ID: `nextest|Test|ovrcr%3A%3Api_delegation|independent_review%3A%3Apublic_review_partial_and_unknown_repairs_keep_original_receipt_without_replay|default`.

Dominant candidates: **Git work and process/PTY spawn/teardown** repeated across three repair failures. Each case makes a coding worker, declares review, launches an independent reviewer and injects partial/unknown input. It then proves recovery and a fresh repair cannot replay bytes (`tests/pi_delegation/review.rs:1554-1633`). Worker creation adds/commits fixture files and real coding work; `axis` launches another assignment (`tests/pi_delegation/review.rs:174-242`). The 10 s/5 ms loop waits only until the actual 7-byte prefix appears; other paths wait for the real child's started marker (`tests/pi_delegation/review.rs:1585-1597`). Immutable candidate/baseline checks remain production behavior (`crates/ovrcr-runtime/src/server/delegation_coding.rs:537-575`).

Fix: commit the common base files once in a private seed repository, then give each scenario its own real coding worktree, reviewer, repair request and fault marker. Reuse the base server only with separate parent/workflow IDs. Keep all actual partial writes, child execution and no-replay assertions. Risk: sharing a consumed repair seat or accepted review across cases can hide budget or identity bugs. A marker that only says “repair called” cannot replace evidence that the exact bytes reached the PTY.

Expected-saving planning range: **0–23.44 s process time**, `35.158 × 2/3` gross repeated-setup envelope; ideal wall 0–7.81/5.86 s at 3/4 slots. Count Git commands and separate worker/reviewer launch, injected write, replay and teardown spans.

### 13. `exclusive_registry_lock_does_not_publish_reopen_success`

Suite: `ovrcr::retained_ownership`. Measured: **34.732 s, 1 attempt**. Definition: `tests/retained_ownership.rs:907-938`. Timing: `research/macos-test-review/evidence/attempts.csv`, matched on the suite and full test name above.

Test ID: `nextest|Test|ovrcr%3A%3Aretained_ownership|exclusive_registry_lock_does_not_publish_reopen_success|default`.

Dominant-cost attribution: **unresolved**. The concrete removable-work candidate is **process spawn and teardown** in repeated CLI inventory calls (`tests/retained_ownership.rs:13-31,149-156`); their share of the 34.732 s is unmeasured. The real `BEGIN EXCLUSIVE` is the test stimulus, not proof that SQLite waiting dominates (`tests/retained_ownership.rs:907-938`). The blocker's 50 ms busy timeout applies to the blocker connection, not the server. Production registry connections use a 1 s busy timeout and FULL synchronization (`crates/ovrcr-runtime/src/config.rs:289-293`); `begin_run` must commit launch intent before process creation (`crates/ovrcr-runtime/src/retained.rs:643-675`). The CLI inspects before reopening (`src/cli/resources.rs:455-471`). **No inspected source establishes a 30 s SQLite timeout responsible for the 34.732 s.** A phase trace must distinguish CLI startup, inspection, storage contention and cleanup before assigning dominance.

Fix: retain the public CLI failure and later successful reopen. Change only supporting inventories inside the locked section from spawned `terminal list` commands (`tests/retained_ownership.rs:149-156`) to direct `live.request(Inspect)` snapshots, asserting the same id/run/PID and process-group equality. Retain one CLI list check after unlocking. This removes incidental CLI startup and possible database-open contention without bypassing the reopen under test. First timestamp CLI inspect, actual reopen request, support snapshots and teardown; verify the failure actually reaches the storage operation rather than failing earlier inspection.

Expected-saving planning range: **0–34.732 s process time** is the only evidence-backed total ceiling; no narrower expected range is supported. Ideal wall ceiling 11.577/8.683 s at 3/4 slots. Never shorten the production lock timeout or disable fsync just to accelerate this test. A phase trace is required before prioritizing the fix over matrix setup.

### 14. `managed_cancel_dispatch_races_retain_intent_without_replay_and_late_id_cancels_exact_run`

Suite: `ovrcr::cursor_cloud`. Measured: **33.503 s, 1 attempt**. Definition: `tests/cursor_cloud.rs:2429-2529`. Timing: `research/macos-test-review/evidence/attempts.csv`, matched on the suite and full test name above.

Test ID: `nextest|Test|ovrcr%3A%3Acursor_cloud|managed_cancel_dispatch_races_retain_intent_without_replay_and_late_id_cancels_exact_run|default`.

Dominant candidate: **process/PTY spawn and teardown** across five paused creation/send cancellation boundaries (`tests/cursor_cloud.rs:2429-2529`). The test waits for a specific checkpoint, durably records cancellation intent, releases that checkpoint and verifies exact CreateAgent/Send/CancelRun counts. The gate uses the production diagnostic 30 s/10 ms checkpoint helper (`crates/ovrcr-runtime/src/cursor_cloud.rs:412-431`; `crates/ovrcr-runtime/src/cursor_cloud/run.rs:604-612`). This is not a consumed 30 s deadline.

Fix: share the server/repository but keep each session, bridge, checkpoint and log slice private. Close/discard completed operations publicly before the next case. Preserve every phase/boundary combination and exact late-ID CancelRun assertion. Risk: replacing the boundary marker with a timing sleep or reusing a run ID would weaken dispatch/cancel ordering and wrong-target protection.

Expected-saving planning range: **0–26.80 s process time**, `33.503 × 4/5`; ideal wall 0–8.93/6.70 s at 3/4 slots. Measure startup/drop spans and gate-hold times independently. The intended gate order is untouchable; the 30 s ceiling is not a savings estimate.

### 15. `claude::public_claude_missing_or_wrong_native_model_never_receives_initial_input`

Suite: `ovrcr::pi_delegation`. Measured: **32.978 s, 1 attempt**. Definition: `tests/pi_delegation/claude.rs:295-317`. Timing: `research/macos-test-review/evidence/attempts.csv`, matched on the suite and full test name above.

Test ID: `nextest|Test|ovrcr%3A%3Api_delegation|claude%3A%3Apublic_claude_missing_or_wrong_native_model_never_receives_initial_input|default`.

Dominant mechanism: **poll loop with a deliberate readiness budget**, plus repeated CLI/provider startup. Two modes provide no native model or the wrong one; both must remain Launched with Accepted initial input and zero bytes (`tests/pi_delegation/claude.rs:295-317`). `ClaudeFixture` starts a full Pi owner, prints Claude setup, configures allowed harnesses, then substitutes a deterministic native host (`tests/pi_delegation/claude.rs:38-83`). The registered production start handler performs up to 100 initial-input calls, sleeping 100 ms whenever still Launched (`src/ovrcr-pi-runtime.ts:340-344`). That is 10 s of scheduled waiting per mode if all 100 iterations remain Launched, plus a CLI spawn per call (`src/ovrcr-pi-runtime.ts:193-201`). Production intentionally leaves a missing/wrong Claude model pending rather than writing (`crates/ovrcr-runtime/src/server/delegation.rs:1778-1800`).

Fix: make production readiness waiting subscribe to exact session/binding/model changes, with the same observation budget and final reconcile result, rather than spawning a CLI and rewriting an unchanged Accepted receipt 100 times. Keep the current public tool path and all zero-byte assertions; preserve CLI transport tests separately when changing the real transport. A smaller test-only count or fake success would weaken coverage. Risk: a subscription must arm before the state check, survive reconnects and reject replacement bindings; otherwise it can lose readiness or send to the wrong generation.

Expected-saving planning range: **0–12.978 s process time** outside the two 10 s polling-sleep budgets, conditional on all iterations having stayed Launched as the final state indicates. The unconditional ceiling is 32.978 s; no trace proves iteration counts. Ideal conditional wall ceiling 4.326/3.245 s at 3/4 slots. Count SendInitial calls and elapsed sleeps first. No saving from shortening the ten-second observation window is proposed.

### 16. `managed_same_bridge_observe_rejects_foreign_changed_missing_and_failed_progress`

Suite: `ovrcr::cursor_cloud`. Measured: **32.603 s, 1 attempt**. Definition: `tests/cursor_cloud.rs:3586-3666`. Timing: `research/macos-test-review/evidence/attempts.csv`, matched on the suite and full test name above.

Test ID: `nextest|Test|ovrcr%3A%3Acursor_cloud|managed_same_bridge_observe_rejects_foreign_changed_missing_and_failed_progress|default`.

Dominant candidate: **process/PTY spawn and teardown** across six same-bridge observation failures, with durable progress writes (`tests/cursor_cloud.rs:3586-3666`). The storage case installs a progress-update trigger; the disconnect case requires two ObserveRun calls, all others one. Validation rejects ambiguous/foreign envelopes and missing run identity (`crates/ovrcr-runtime/src/cursor_cloud/run.rs:32-79`). Polling terminal text uses the shared 10 ms/15 s helper, not a deliberate sixfold 15 s wait (`tests/cursor_cloud.rs:125-135`).

Fix: reuse the server/repository and recreate the bridge/session per scenario, with per-case event offsets and public cleanup. Scope the storage trigger to its case and prove it has been dropped. Preserve exactly one Send, exact ObserveRun counts, no Resume/ListRuns and no ready/input/unread state. Risk: caching a progress cursor or leaving a trigger installed could turn the next scenario into a different failure and still produce “unknown.”

Expected-saving planning range: **0–27.17 s process time**, `32.603 × 5/6`; ideal wall 0–9.06/6.79 s at 3/4 slots. Trace bridge startup, observation attempts, trigger failure and cleanup before predicting savings.

### 17. `managed_send_storage_faults_never_dispatch_without_claim_or_retire_unreceipted_payload`

Suite: `ovrcr::cursor_cloud`. Measured: **30.996 s, 1 attempt**. Definition: `tests/cursor_cloud.rs:1796-1937`. Timing: `research/macos-test-review/evidence/attempts.csv`, matched on the suite and full test name above.

Test ID: `nextest|Test|ovrcr%3A%3Acursor_cloud|managed_send_storage_faults_never_dispatch_without_claim_or_retire_unreceipted_payload|default`.

Dominant candidates: **process/PTY spawn/teardown and SQLite durability**, six fresh fixtures around Send-storage faults (`tests/cursor_cloud.rs:1796-1937`). Trigger conditions target intent, claim, receipt and terminal commits; corrupt/closure cases pause before dispatch. The test checks exact Send counts, retained payloads/redaction and Unknown activity when terminal storage fails. Production initial storage commits are part of correctness; retained launch state is durable-before-effect (`crates/ovrcr-runtime/src/retained.rs:643-675`), and Send checkpoints reuse the named boundary helper (`crates/ovrcr-runtime/src/cursor_cloud/run.rs:604-612`).

Fix: reuse the repository/server across cases only after public discard removes each payload and every case trigger is dropped. Always recreate bridge/session and database trigger conditions. Keep actual on-disk SQLite and all abort triggers; do not use in-memory storage or turn synchronous mode off. Risk: sharing a transaction or rolling the case back wholesale would avoid the very failed-commit boundary being tested.

Expected-saving planning range: **0–25.83 s process time**, `30.996 × 5/6`; ideal wall 0–8.61/6.46 s at 3/4 slots. Measure fixture and fsync/SQL spans separately. This range does not claim fsync itself can be removed.

### 18. `public_dispatch_recovers_lost_requests_and_replies_without_duplicate_effects`

Suite: `ovrcr::pi_delegation`. Measured: **29.755 s, 1 attempt**. Definition: `tests/pi_delegation.rs:2366-2420`. Timing: `research/macos-test-review/evidence/attempts.csv`, matched on the suite and full test name above.

Test ID: `nextest|Test|ovrcr%3A%3Api_delegation|public_dispatch_recovers_lost_requests_and_replies_without_duplicate_effects|default`.

Dominant candidate: **process/PTY spawn and teardown**, six full owner/child fixtures for Prepare/Launch/InitialInput request-versus-reply loss (`tests/pi_delegation.rs:2366-2420`). The proxy forwards or drops exact original frames; it is serial to preserve mutation ordering (`tests/pi_delegation.rs:1240-1289`). The test recovers via registered tools, waits for actual child input, then checks one child, one input and immutable replay/conflict behavior. Production stores original input authority before effect (`crates/ovrcr-runtime/src/server/delegation.rs:1691-1747`).

Fix: hoist server and base repo while creating an independent managed parent/workflow per case; reset proxy rules and request history for each case. Keep six real frame-loss cases and direct replay of original allocated identities. Risk: a shared request history could return a previous case's original frame; require case-specific handles and assert the proxy's matched operation/handle before each replay.

Expected-saving planning range: **0–24.80 s process time**, `29.755 × 5/6`; ideal wall 0–8.27/6.20 s at 3/4 slots. Time parent setup, child readiness, fault round trip and cleanup, rather than blaming the 20 s wait-file ceiling.

### 19. `public_pi_admission_uses_final_slot_and_allows_more_than_four_children`

Suite: `ovrcr::retained_capacity`. Measured: **28.421 s, 1 attempt**. Definition: `tests/retained_capacity.rs:387-721`. Timing: `research/macos-test-review/evidence/attempts.csv`, matched on the suite and full test name above.

Test ID: `nextest|Test|ovrcr%3A%3Aretained_capacity|public_pi_admission_uses_final_slot_and_allows_more_than_four_children|default`.

Dominant candidate: **process/PTY spawn and teardown**, with avoidable repeated inventory work around the required 50 live sessions (`tests/retained_capacity.rs:387-721`). Five delegated children and parent/shell give seven live sessions; the filler loop creates 42 more, then the public admission/race consumes the final slot. The limit of 50 is production behavior (`crates/ovrcr-runtime/src/server/mod.rs:223-223,1141-1157`). Each filler currently calls `track_groups`, rescanning all sessions and `getpgid` values (`tests/retained_capacity.rs:36-39,482-486`; `tests/support/live.rs:477-489`).

Fix: immediately register only the just-created session's verified process group from its returned PID (`tests/retained_capacity.rs:93-104,120-123`) instead of rescanning every existing group. Keep live-count checks, the Paused-slot assertion, actual 50-process boundary, final barrier race and preservation of all existing session identities. With exactly seven initial live sessions, the filler rescans cover 8+…+49 = 1,197 live group lookups; 42 direct registrations avoid about 1,155 redundant lookups, ignoring any exited/nonlive rows. Risk: registration must happen before any following assertion can panic; do not defer all cleanup tracking until the end.

Expected-saving planning range: **0–28.421 s process time** is a gross total ceiling; no per-lookup cost supports a narrower seconds estimate. Ideal wall ceiling 9.474/7.105 s at 3/4 slots. Measure counts and elapsed inventory/getpgid work. Reducing 50 to a small test limit or replacing PTYs with fake rows would lose the capacity guarantee.

### 20. `forced_cancel_kills_pi_process_group`

Suite: `ovrcr::task_execution`. Measured: **28.334 s, 1 attempt**. Definition: `tests/task_execution.rs:232-291`. Timing: `research/macos-test-review/evidence/attempts.csv`, matched on the suite and full test name above.

Test ID: `nextest|Test|ovrcr%3A%3Atask_execution|forced_cancel_kills_pi_process_group|default`.

Dominant mechanism: **deliberate cancellation deadline budget**. The test waits for the detached child to be recorded, SIGSTOPs its supervisor, cancels publicly and waits for CleanupFailed, then proves all PIDs/groups are gone (`tests/task_execution.rs:232-291`). Production waits more than 20 s after sending cancel, sends SIGTERM, allows 5 s for supervisor cleanup, then SIGKILLs and cleans recorded descendants (`crates/ovrcr-runtime/src/task_manager.rs:378-409`). The test's 40 s limit is only a watchdog; the 300 s sleep child is killed, not waited out (`tests/task_execution.rs:49-53`).

Fix: retain the real 20+5 s escalation path and SIGSTOP. Replace repeated CLI `run get` progress polling with notifications/read-only observation of the exact run-state file, then perform the final public CLI get and all current CleanupFailed/PID/group assertions (`tests/task_execution.rs:107-124`). Do not shorten cancellation budgets in production or delete the frozen-supervisor case. Risk: canceling before `processes.json` contains the detached child would no longer test recorded-descendant cleanup.

Expected-saving planning range: **0–3.334 s process time** outside the expected forced 25 s escalation (`28.334−25`), conditional on the frozen supervisor consuming that full grace. Ideal wall 0–1.111/0.834 s at 3/4 slots. Timestamp cancel send, TERM, KILL, recorded cleanup and CLI polling to confirm the mandatory floor.

### 21. `managed_creation_storage_failures_permit_only_durably_claimed_effects`

Suite: `ovrcr::cursor_cloud`. Measured: **27.911 s, 1 attempt**. Definition: `tests/cursor_cloud.rs:851-984`. Timing: `research/macos-test-review/evidence/attempts.csv`, matched on the suite and full test name above.

Test ID: `nextest|Test|ovrcr%3A%3Acursor_cloud|managed_creation_storage_failures_permit_only_durably_claimed_effects|default`.

Dominant candidates: **process/PTY spawn/teardown and SQLite durability** across six creation-storage failure modes (`tests/cursor_cloud.rs:851-984`). Intent/claim/receipt/closure triggers, corrupt request bytes and a symlinked private root test distinct fail-closed boundaries. Marker waits are event conditions; production's 30 s checkpoint ceiling is not normally consumed (`crates/ovrcr-runtime/src/cursor_cloud.rs:412-431`).

Fix: share server/repository only after case-specific public cleanup proves no payload or live session remains. Recreate each bridge/session; drop triggers and remove the symlink before reuse, preserving zero-or-one CreateAgent and no Send assertions. A simpler low-risk alternative is sharing only an immutable Git seed and retaining separate databases. Risk: the corrupt/private-root cases change storage identity, so reusing a poisoned directory or trigger can make later cases pass for the wrong reason. Keep all real SQL failures and redaction checks.

Expected-saving planning range: **0–23.26 s process time**, `27.911 × 5/6`; ideal wall 0–7.75/5.81 s at 3/4 slots. Measure startup and teardown separately from storage fault/release work. Do not reduce durability to improve the number.

### 22. `public_dispatch_freezes_fresh_child_target_before_metrics_and_write`

Suite: `ovrcr::pi_delegation`. Measured: **27.099 s, 1 attempt**. Definition: `tests/pi_delegation.rs:2185-2353`. Timing: `research/macos-test-review/evidence/attempts.csv`, matched on the suite and full test name above.

Test ID: `nextest|Test|ovrcr%3A%3Api_delegation|public_dispatch_freezes_fresh_child_target_before_metrics_and_write|default`.

Dominant candidates: **process/PTY startup plus poll loops** in three capture/rebind stages (`tests/pi_delegation.rs:2185-2353`). It freezes an initial child target, drives A→B→A generation changes, and proves no byte/input replay. The 5 ms poll exits only when binding generation is 3. The child fixture polls the rebind file at 5 ms and emits both actual replacement callbacks (`tests/fixtures/pi/pi_host.mjs:150-160`). The after-capture case starts with a waiting model, so production's repeated SendInitial/100 ms readiness loop can also matter (`src/ovrcr-pi-runtime.ts:340-344`).

Fix: notify the test when both rebind callbacks have completed, then inspect and assert generation 3; retain the named before-first/after-capture/before-write gates. Share a server only across three independent parents/workflows with fresh proxy histories. Optimize repeated production CLI control calls only with the same exact-binding authorization semantics. Risk: a mere “gate file exists” condition is too early; an apparent A conversation can be stale generation 1 rather than returned generation 3.

Expected-saving planning range: **0–18.07 s process time**, `27.099 × 2/3` gross setup envelope. It is not additive with any readiness-transport change. Ideal wall 0–6.02/4.52 s at 3/4 slots. Count readiness iterations and measure each fault hold; no saving from shrinking the rebind/race observation budget is proposed.

### 23. `claude_reopen_retains_exact_conversation_before_another_callback`

Suite: `ovrcr::retained_sessions`. Measured: **26.851 s, 1 attempt**. Definition: `tests/retained_sessions.rs:1123-1127`. Timing: `research/macos-test-review/evidence/attempts.csv`, matched on the suite and full test name above.

Test ID: `nextest|Test|ovrcr%3A%3Aretained_sessions|claude_reopen_retains_exact_conversation_before_another_callback|default`.

Dominant candidate: **process/PTY spawn/restart and teardown**, amplified by spawned CLI polling. Two initial-resume variants each run full Claude retained recovery, including two successful restart/reopen cycles, missing-resource refusals, durable title/conversation checks and final clear/invalidation (`tests/retained_sessions.rs:1123-1127,1149-1330`). `wait_output` repeatedly starts a CLI and calls `yield_now` between attempts (`tests/retained_sessions.rs:1105-1119`); it has no 10 ms backoff. Production retained conversation writes and run recovery are real (`crates/ovrcr-runtime/src/retained.rs:561-590`; `crates/ovrcr-runtime/src/server/mod.rs:1476-1512`).

Fix: await a terminal output event or use the existing socket ReadTerminal API with bounded 5–10 ms polling, then perform one CLI `terminal read` to assert the same marker. Keep both variants, real restarts, SQLite trigger failures, legacy migration and all final argv/callback checks. This removes repeated observer CLI startup without removing public CLI coverage of each observed marker. Risk: callback acknowledgement and retained-state commit are distinct; wait for the exact marker/state the original helper checks, not mere native process existence.

Expected-saving planning range: **0–26.851 s process time** is the only measured ceiling for this change; CLI-start counts and durations are absent. Ideal wall ceiling 8.950/6.713 s at 3/4 slots. Measure CLI attempts per marker before selecting a target. Do not present half the test as removable just because it has two variants.

### 24. `questions::public_question_sixty_four_answer_effects_exhaust_storage_without_eviction_or_replay`

Suite: `ovrcr::pi_delegation`. Measured: **26.727 s, 1 attempt**. Definition: `tests/pi_delegation/questions.rs:867-962`. Timing: `research/macos-test-review/evidence/attempts.csv`, matched on the suite and full test name above.

Test ID: `nextest|Test|ovrcr%3A%3Api_delegation|questions%3A%3Apublic_question_sixty_four_answer_effects_exhaust_storage_without_eviction_or_replay|default`.

Dominant candidates: **SQLite/fsync durability and repeated registered-tool control calls** over 64 real answers. Each answer must be independently consumed by the child; all 64 receipts, original answer bytes and replay behavior remain asserted (`tests/pi_delegation/questions.rs:867-962`). Helpers use reply-file waits and atomic command files (`tests/pi_delegation/questions.rs:49-96`). The child consumes actual scoped input and records an answer count (`tests/fixtures/pi/pi_host.mjs:230-238`), while its question-command loop polls every 5 ms (`tests/fixtures/pi/pi_host.mjs:322-367`). Production checks answer capacity and durably saves around delivery (`crates/ovrcr-runtime/src/server/delegation_questions.rs:686-689,784-786,875-919`).

Fix: add fixture notifications for exact command completion, tool reply and child answer consumption. Still read/assert the on-disk answer and count after notification. Retain all 64 public answers and the final archive/history checks; do not seed 63 answers in SQLite. Risk: the count file is written before the answer-received file, so notification ordering must keep the latter fully written before returning it to the test.

Expected-saving planning range: **0–2.545 s process time** of observation latency: up to 63×10 ms command reply waits + 64×10 ms answer tool waits + 64×5 ms child-consumption waits + 64×10 ms received-file waits + 63×5 ms child command detection. This intentionally generous bound may count overlapping waits; do not add it to other changes on this test. Ideal wall 0–0.848/0.636 s at 3/4 slots. Measure each completion-to-observation lag; SQL/publication time remains.

### 25. `extension_conversations_switch_and_survive_repeated_restart`

Suite: `ovrcr::retained_sessions`. Measured: **25.901 s, 1 attempt**. Definition: `tests/retained_sessions.rs:574-974`. Timing: `research/macos-test-review/evidence/attempts.csv`, matched on the suite and full test name above.

Test ID: `nextest|Test|ovrcr%3A%3Aretained_sessions|extension_conversations_switch_and_survive_repeated_restart|default`.

Dominant candidate: **process/PTY spawn/restart and teardown plus spawned CLI polling** over two providers (`tests/retained_sessions.rs:574-974`). Each provider performs conversation A/B/A changes, retired-producer rejection, retention trigger failure, real owner departure, old-run authenticated delayed writes, two restart/reopen cycles and an ephemeral-history refusal. These are distinct behavior boundaries, not redundant provider names. Output/phase helpers repeatedly spawn CLI processes without a timed backoff (`tests/retained_sessions.rs:1064-1082,1105-1119`). Production stores the exact conversation on disk and rejects run changes (`crates/ovrcr-runtime/src/retained.rs:561-590`); reopen terminates an old session before creating its successor (`crates/ovrcr-runtime/src/server/mod.rs:1643-1651`).

Fix: use socket output/state notifications for progress and retain one final CLI read/list assertion at each observed marker. Keep provider-private histories, all actual restarts, old-run capability delivery, argv equality and secret absence checks. Seed the immutable repository once only if each provider still receives a private checkout and database. Risk: replacing owner-departure evidence with a “shutdown sent” acknowledgement would lose lease-retirement and stale-capability protection.

Expected-saving planning range: **0–25.901 s process time** is the gross measured ceiling; no observer-spawn counts support a tighter estimate. Ideal wall ceiling 8.634/6.475 s at 3/4 slots. Trace per-provider setup, CLI observation spawns, each mandatory restart and cleanup. Do not remove the second restart or either provider to make it faster.

## Do not touch: deliberate safety budgets and load-bearing races

“Do not touch” means do not shorten the budget, remove the real boundary or replace the state proof with a faster guess. Fixture construction and observer overhead can still be optimized as described above.

- **The two 60 s quota deadline tests (ranks 2–3).** Keep both modes and the elapsed-time/PID-absence assertions. One catches silent expiry; the other catches startup callbacks incorrectly completing the probe before usage. A fake clock test already exists for cadence (`tests/claude_quota_probe.rs:393-448`), but it does not replace real process expiry (`tests/claude_quota_probe.rs:351-389`; `crates/ovrcr-runtime/src/server/quota_probe.rs:18-18,317-394`).
- **Expired notification permission (rank 11).** Keep the real 30 s production polling budget and the 1.2 s post-expiry silence window. They protect bounded automatic polling and recovery without reauthorization. A sub-second negative check could miss the next one-second status poll (`tests/server_lifecycle.rs:18475-18509`; `crates/ovrcr-tui/src/dashboard/desktop.rs:541-566`).
- **Forced cancellation (rank 20).** Keep SIGSTOP, proof that the detached child was recorded, the 20 s cancellation deadline, 5 s TERM grace, forced kill and full process-group absence. This exercises an unresponsive supervisor, not ordinary cooperative cancellation (`tests/task_execution.rs:251-290`; `crates/ovrcr-runtime/src/task_manager.rs:378-409`).
- **Missing/wrong Claude native model (rank 15).** Keep a full bounded observation of the actual public tool and zero native input. Event-driven waiting can remove repeated control-process work, but a tiny test-only retry count would stop probing whether input leaks while readiness remains invalid (`tests/pi_delegation/claude.rs:295-317`; `src/ovrcr-pi-runtime.ts:340-344`; `crates/ovrcr-runtime/src/server/delegation.rs:1778-1800`).
- **Exact target rebind, partial input, cancellation and crash checkpoints (ranks 6, 7, 12, 14, 22).** Keep named pre-effect/post-claim/post-reply boundaries and real byte-count/receipt proofs. These tests do not normally consume their full 5/10/20/30 s watchdogs. Lowering watchdogs cannot reduce a passing run, but can add failures on hosted runners. A readiness event may replace polling only after the same exact state transition has happened (`tests/pi_delegation.rs:2185-2353`; `tests/pi_delegation/recovery.rs:801-911`; `tests/pi_delegation/review.rs:1585-1629`; `tests/cursor_cloud.rs:1940-2050,2429-2529`; `crates/ovrcr-runtime/src/cursor_cloud.rs:412-431`).
- **OAuth slowdown interval.** `device_slowdown_without_interval_adds_five_seconds` asserts at least six seconds between the slow_down response and token poll. That wait is the authentication contract, not fixture slack. Removing it permits too-frequent authorization polling (`tests/operator_auth.rs:425-450`). A virtual-clock companion would not, by itself, preserve this real HTTP/timer acceptance boundary.
- **Real owner-hash authentication and its scheduler reservation.** Keep the fixed 1,500 ms owner authentication budget and the exclusive scheduling until a replacement proves the same real-owner behavior under contention. The configuration explicitly explains that reservation (`.config/nextest.toml:19-22`), and the palette test authenticates the actual current dashboard/socket/owner (`tests/server_lifecycle.rs:20377-20425`). Removing it trades away the quiet-run condition that makes that bounded real-process proof reliable. The other three exclusive click tests also need their real callback/PTY race analyzed before lifting isolation (`.config/nextest.toml:15-17`); their 27.658 s is not automatic wall savings.
- **Negative “nothing happened” windows in titles and settings.** Do not replace a 3/5 s settle with “the call file does not yet exist.” That can pass before the asynchronous worker runs. A behavior-preserving replacement must acknowledge that the relevant title/settings scan completed and all owned work settled, then retain the no-call/no-new-event assertion (`tests/automatic_titles.rs:763-783,2050-2062,2177-2221`; `tests/server_lifecycle.rs:20218-20258`). A positive success marker from an unrelated session is insufficient.
- **50-session admission and 64-entry history tests (ranks 8, 19, 24).** These costs are deliberate capacity/effect coverage even though they are not clock budgets. Keep 50 actual live sessions, every 64 followup/answer effect, the last allowed operation and first refused operation. Smaller configured limits or direct SQL seeding trade away the product's real ceiling and the accumulated on-disk/publication/receipt history (`tests/retained_capacity.rs:463-528,667-720`; `tests/pi_delegation/followups.rs:545-599`; `tests/pi_delegation/questions.rs:875-958`).
- **Real locked and durable storage (ranks 5, 8, 9, 13, 17, 21, 24).** Keep real SQLite failures/locks, on-disk operation files and FULL synchronization. Turning durability down or replacing failed commits with mocked errors no longer proves that no process/network effect escapes before a committed claim (`crates/ovrcr-runtime/src/config.rs:289-293`; `crates/ovrcr-runtime/src/retained.rs:643-675`; `tests/retained_ownership.rs:917-937`; `tests/cursor_cloud.rs:869-938,1830-1875`).

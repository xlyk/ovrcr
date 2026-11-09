# Test cost versus value

Revision `992a1da5c59a865ebb48051e4d9d8ff7184c8dc5`; run 42429838269 job 128576086691. Every number is aggregated from the named evidence files; the derivations are repeated in `cost-analysis.json` (`derivation` fields and the `rule` strings).

Inputs: `evidence/attempts.csv` (2822 nextest attempt rows), `evidence/inventory.csv` (3171 rows), `evidence/job-steps.csv` (23 steps), `evidence/node-tap.txt`, `evidence/macos-job.log`, the 50 `reviews/*.jsonl` rows (3171) and the 11 `evidence/quality-review-*.jsonl` receipts (450 rows: 405 agree, 45 resolved). A review score is replaced by a receipt’s `final_review` only where the verdict is `resolved`.

## 1. Total picture

| Quantity | Value | Source / aggregation |
|---|---:|---|
| Step-covered interval | 2415 s | `job-steps.csv`: 2026-11-08T00:27:42Z to 01:07:57Z |
| Job-record wall time | 2418 s | `run.json`: 00:27:42Z to 01:08:00Z |
| Main `Test` step | 2154 s | `job-steps.csv` step `Test` |
| Compilation inside Test | 136 s | Test notes; log 502 |
| Nextest execution | 2014.918 s | Test notes; log 505 to log 3397 |
| Test-step residual | 3.082 s | 2154 minus (136 + 2014.918) |
| Cumulative test-process time | 5931.968 s | sum of `attempts.csv.seconds` over 2822 rows; equals sum of `inventory.csv.total_attempt_seconds` over 2820 `runner=nextest` rows |
| Nextest tests / binaries / skipped | 2820 / 56 / 33 | Test notes |
| Attempt rows (PASS/FAIL) | 2822 (2820/2) | `attempts.csv` |

Per-test process durations were measured concurrently by nextest and by parallel CI steps; their sum (5931.968 s) is cumulative process time, not wall-clock time and not a speedup that could be recovered by removing tests.

Other test steps are outside the 5931.968 s nextest total: `Doc tests` 49 s (five sections, 0 tests), `Extension tests` 5 s, installed-Pi materialization 31 s and installed-Pi child tools 16 s.

## 2. Cost groups by suite (binary_id)

Rule: group `attempts.csv` by `binary_id`; seconds = sum of attempt `seconds`; rows = attempt-row count; share = seconds / 5931.968.

| # | binary_id | seconds | attempt rows | share |
|---:|---|---:|---:|---:|
| 1 | ovrcr::pi_delegation | 2380.841 | 260 | 40.14% |
| 2 | ovrcr::server_lifecycle | 764.781 | 157 | 12.89% |
| 3 | ovrcr::cursor_cloud | 702.799 | 47 | 11.85% |
| 4 | ovrcr::automatic_titles | 307.713 | 34 | 5.19% |
| 5 | ovrcr::retained_sessions | 219.666 | 22 | 3.70% |
| 6 | ovrcr::claude_quota_probe | 167.450 | 14 | 2.82% |
| 7 | ovrcr-runtime | 154.868 | 496 | 2.61% |
| 8 | ovrcr::terminal_acceptance | 154.485 | 34 | 2.60% |
| 9 | ovrcr::provider_quota | 134.734 | 33 | 2.27% |
| 10 | ovrcr::gui | 99.716 | 13 | 1.68% |
| 11 | ovrcr::retained_ownership | 94.812 | 13 | 1.60% |
| 12 | ovrcr::task_execution | 87.274 | 13 | 1.47% |
| 13 | ovrcr::cli | 66.545 | 50 | 1.12% |
| 14 | ovrcr::client_startup_server | 61.644 | 21 | 1.04% |
| 15 | ovrcr::retained_capacity | 57.052 | 5 | 0.96% |

The top 3 binaries account for **3848.421 s (64.88%)**: `ovrcr::pi_delegation` 2380.841 s, `ovrcr::server_lifecycle` 764.781 s, `ovrcr::cursor_cloud` 702.799 s. The top 15 account for 5454.380 s (91.95%); the remaining 41 binaries account for 477.588 s, and the total is 5931.968 s.

## 3. Slowest 30 individual tests

Rule: `inventory.csv` `runner=nextest`, ordered by `total_attempt_seconds` desc then full `test_id` asc. Scores are the effective review scores; `receipt` marks a `final_review` override. The 30 sum to 1089.436 s (18.37% of the cumulative total).

| # | test_id | source | s | att | C | S | U | F | rec | receipt | cost driver |
|---:|---|---|---:|---:|---:|---:|---:|---:|---|---|---|
| 1 | `nextest\|Test\|ovrcr%3A%3Acursor_cloud\|managed_cloud_refuses_incompatible_bridge_before_account_requests\|default` | `tests/cursor_cloud.rs` | 68.639 | 1 | 3 | 4 | 4 | 3 | keep | no | thirteen fixtures plus PTY sessions |
| 2 | `nextest\|Test\|ovrcr%3A%3Aclaude_quota_probe\|claude_probe_startup_without_usage_reaches_the_deadline_and_the_process_is_gone\|default` | `tests/claude_quota_probe.rs` | 62.920 | 1 | 3 | 3 | 3 | 3 | keep | no | 60 s real deadline |
| 3 | `nextest\|Test\|ovrcr%3A%3Aclaude_quota_probe\|claude_probe_silent_for_sixty_seconds_times_out_and_the_process_is_gone\|default` | `tests/claude_quota_probe.rs` | 62.912 | 1 | 3 | 4 | 3 | 3 | keep | no | 60 s real deadline per run |
| 4 | `nextest\|Test\|ovrcr%3A%3Acursor_cloud\|managed_send_uncertainty_and_malformed_evidence_never_retry_or_become_ready\|default` | `tests/cursor_cloud.rs` | 56.663 | 1 | 3 | 4 | 4 | 3 | keep | no | nine PTY sessions |
| 5 | `nextest\|Test\|ovrcr%3A%3Api_delegation\|dependencies%3A%3Apublic_dependency_lost_requests_and_replies_reconcile_original_baseline_without_duplicate_effects\|default` | `tests/pi_delegation/dependencies.rs` | 55.590 | 1 | 3 | 4 | 3 | 4 | keep | no | real git worktrees |
| 6 | `nextest\|Test\|ovrcr%3A%3Api_delegation\|parent_recovery%3A%3Aparent_recovery_transfer_discovers_prepared_and_suspends_accepted_unknown_partial_effects\|default` | `tests/pi_delegation/recovery.rs` | 46.250 | 2 | 3 | 4 | 4 | 4 | keep | no | real managed PTY |
| 7 | `nextest\|Test\|ovrcr%3A%3Acursor_cloud\|managed_send_crash_and_discard_boundaries_preserve_uncertainty_and_exact_late_identity\|default` | `tests/cursor_cloud.rs` | 46.006 | 1 | 3 | 4 | 4 | 3 | keep | no | eight PTY sessions |
| 8 | `nextest\|Test\|ovrcr%3A%3Api_delegation\|followups%3A%3Apublic_followup_storage_limit_preserves_last_result_acceptance_and_all_receipts\|default` | `tests/pi_delegation/followups.rs` | 42.056 | 1 | 3 | 4 | 4 | 4 | keep | no | real managed PTY |
| 9 | `nextest\|Test\|ovrcr%3A%3Acursor_cloud\|managed_creation_crashes_retain_original_operation_and_require_explicit_discard\|default` | `tests/cursor_cloud.rs` | 39.866 | 1 | 3 | 4 | 4 | 3 | keep | no | four restarts |
| 10 | `nextest\|Test\|ovrcr%3A%3Acursor_cloud\|managed_recovery_account_source_and_unknown_outcomes_fail_closed\|default` | `tests/cursor_cloud.rs` | 37.103 | 1 | 3 | 4 | 4 | 3 | keep | no | six PTY sessions |
| 11 | `nextest\|Test\|ovrcr%3A%3Aserver_lifecycle\|desktop_notifications_bridge_expired_permission_check_recovers_without_reauthorization\|default` | `tests/server_lifecycle.rs` | 35.784 | 1 | 2 | 3 | 3 | 4 | keep | no | 36s wall-clock budget |
| 12 | `nextest\|Test\|ovrcr%3A%3Api_delegation\|independent_review%3A%3Apublic_review_partial_and_unknown_repairs_keep_original_receipt_without_replay\|default` | `tests/pi_delegation/review.rs` | 35.158 | 1 | 3 | 4 | 3 | 4 | keep | no | real git worktrees |
| 13 | `nextest\|Test\|ovrcr%3A%3Aretained_ownership\|exclusive_registry_lock_does_not_publish_reopen_success\|default` | `tests/retained_ownership.rs` | 34.732 | 1 | 3 | 3 | 3 | 4 | keep | no | real sqlite lock |
| 14 | `nextest\|Test\|ovrcr%3A%3Acursor_cloud\|managed_cancel_dispatch_races_retain_intent_without_replay_and_late_id_cancels_exact_run\|default` | `tests/cursor_cloud.rs` | 33.503 | 1 | 3 | 4 | 4 | 3 | keep | no | five paused boundaries |
| 15 | `nextest\|Test\|ovrcr%3A%3Api_delegation\|claude%3A%3Apublic_claude_missing_or_wrong_native_model_never_receives_initial_input\|default` | `tests/pi_delegation/claude.rs` | 32.978 | 1 | 3 | 4 | 3 | 3 | keep | no | scripted Claude host |
| 16 | `nextest\|Test\|ovrcr%3A%3Acursor_cloud\|managed_same_bridge_observe_rejects_foreign_changed_missing_and_failed_progress\|default` | `tests/cursor_cloud.rs` | 32.603 | 1 | 3 | 4 | 4 | 3 | keep | no | six PTY sessions |
| 17 | `nextest\|Test\|ovrcr%3A%3Acursor_cloud\|managed_send_storage_faults_never_dispatch_without_claim_or_retire_unreceipted_payload\|default` | `tests/cursor_cloud.rs` | 30.996 | 1 | 3 | 4 | 4 | 3 | keep | no | six fixtures with DB triggers |
| 18 | `nextest\|Test\|ovrcr%3A%3Api_delegation\|public_dispatch_recovers_lost_requests_and_replies_without_duplicate_effects\|default` | `tests/pi_delegation.rs` | 29.755 | 1 | 3 | 4 | 3 | 4 | keep | no | fault proxy frame loss |
| 19 | `nextest\|Test\|ovrcr%3A%3Aretained_capacity\|public_pi_admission_uses_final_slot_and_allows_more_than_four_children\|default` | `tests/retained_capacity.rs` | 28.421 | 1 | 4 | 4 | 4 | 3 | keep | no | 50-session matrix |
| 20 | `nextest\|Test\|ovrcr%3A%3Atask_execution\|forced_cancel_kills_pi_process_group\|default` | `tests/task_execution.rs` | 28.334 | 1 | 3 | 4 | 3 | 4 | keep | no | A 40s deadline, SIGSTOP and real process-group probes |
| 21 | `nextest\|Test\|ovrcr%3A%3Acursor_cloud\|managed_creation_storage_failures_permit_only_durably_claimed_effects\|default` | `tests/cursor_cloud.rs` | 27.911 | 1 | 3 | 4 | 4 | 3 | keep | no | six fixtures with DB triggers |
| 22 | `nextest\|Test\|ovrcr%3A%3Api_delegation\|public_dispatch_freezes_fresh_child_target_before_metrics_and_write\|default` | `tests/pi_delegation.rs` | 27.099 | 1 | 3 | 4 | 3 | 4 | keep | no | acceptance-diagnostics feature |
| 23 | `nextest\|Test\|ovrcr%3A%3Aretained_sessions\|claude_reopen_retains_exact_conversation_before_another_callback\|default` | `tests/retained_sessions.rs` | 26.851 | 1 | 4 | 3 | 3 | 3 | keep | no | real restarts+sqlite |
| 24 | `nextest\|Test\|ovrcr%3A%3Api_delegation\|questions%3A%3Apublic_question_sixty_four_answer_effects_exhaust_storage_without_eviction_or_replay\|default` | `tests/pi_delegation/questions.rs` | 26.727 | 1 | 3 | 4 | 4 | 4 | keep | no | real managed PTY |
| 25 | `nextest\|Test\|ovrcr%3A%3Aretained_sessions\|extension_conversations_switch_and_survive_repeated_restart\|default` | `tests/retained_sessions.rs` | 25.901 | 1 | 4 | 4 | 4 | 4 | keep | no | real restarts |
| 26 | `nextest\|Test\|ovrcr%3A%3Acursor_cloud\|managed_live_cancel_known_run_requires_durable_intent_and_authoritative_outcome\|default` | `tests/cursor_cloud.rs` | 24.248 | 1 | 3 | 4 | 4 | 3 | keep | no | four PTY sessions |
| 27 | `nextest\|Test\|ovrcr%3A%3Aautomatic_titles\|three_failed_title_attempts_close_window_and_survive_restart\|default` | `tests/automatic_titles.rs` | 22.799 | 1 | 3 | 4 | 3 | 3 | keep | no | provider exits 2 per call |
| 28 | `nextest\|Test\|ovrcr%3A%3Acursor_cloud\|ordinary_cloud_agent_validates_preview_and_cleans_owned_tree_on_exit_and_signals\|default` | `tests/cursor_cloud.rs` | 22.621 | 1 | 3 | 4 | 4 | 3 | keep | no | four PTY sessions |
| 29 | `nextest\|Test\|ovrcr%3A%3Api_delegation\|fallback%3A%3Adraft_public_same_run_old_future_unrelated_and_truncated_markers_stay_incomplete\|default` | `tests/pi_delegation/fallback.rs` | 22.588 | 1 | 3 | 4 | 4 | 4 | keep | no | real managed PTY |
| 30 | `nextest\|Test\|ovrcr%3A%3Api_delegation\|observation%3A%3Aproduction_observer_deadline_reconnect_overflow_and_wait_abort_leave_child_running\|default` | `tests/pi_delegation/observation.rs` | 22.422 | 1 | 3 | 4 | 4 | 4 | keep | no | real managed PTY |

Receipt overrides among the 30: 0.

## 4. Expensive and valuable versus expensive and weak

Rules: valuable = C >= 3; weak = C <= 2 **and** (S <= 2 or U <= 1 or recommendation in {consolidate, prune_candidate}).

- Expensive and valuable: **29 of 30** (ranks 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30).
- Expensive and weak: **0 of 30** — no row meets the weak rule.
- Neither: **1** (rank 11). Rank 11 (desktop_notifications_bridge_expired_permission_check_recovers_without_reauthorization) is the only C<=2 row in the 30; C2/S3/U3/F4/keep makes it neither weak nor valuable by this rule. No speedup is claimed for either bucket.

What makes the valuable 29 slow, from their recorded `cost_drivers` (fixture or deadline, not a measured cause):

- rank 1 `managed_cloud_refuses_incompatible_bridge_before_account_requests`: thirteen fixtures plus PTY sessions
- rank 2 `claude_probe_startup_without_usage_reaches_the_deadline_and_the_process_is_gone`: 60 s real deadline
- rank 3 `claude_probe_silent_for_sixty_seconds_times_out_and_the_process_is_gone`: 60 s real deadline per run
- rank 4 `managed_send_uncertainty_and_malformed_evidence_never_retry_or_become_ready`: nine PTY sessions
- rank 5 `dependencies::public_dependency_lost_requests_and_replies_reconcile_original_baseline_without_duplicate_effects`: real git worktrees
- rank 6 `parent_recovery::parent_recovery_transfer_discovers_prepared_and_suspends_accepted_unknown_partial_effects`: real managed PTY
- rank 7 `managed_send_crash_and_discard_boundaries_preserve_uncertainty_and_exact_late_identity`: eight PTY sessions
- rank 8 `followups::public_followup_storage_limit_preserves_last_result_acceptance_and_all_receipts`: real managed PTY
- rank 9 `managed_creation_crashes_retain_original_operation_and_require_explicit_discard`: four restarts
- rank 10 `managed_recovery_account_source_and_unknown_outcomes_fail_closed`: six PTY sessions
- rank 12 `independent_review::public_review_partial_and_unknown_repairs_keep_original_receipt_without_replay`: real git worktrees
- rank 13 `exclusive_registry_lock_does_not_publish_reopen_success`: real sqlite lock
- rank 14 `managed_cancel_dispatch_races_retain_intent_without_replay_and_late_id_cancels_exact_run`: five paused boundaries
- rank 15 `claude::public_claude_missing_or_wrong_native_model_never_receives_initial_input`: scripted Claude host
- rank 16 `managed_same_bridge_observe_rejects_foreign_changed_missing_and_failed_progress`: six PTY sessions
- rank 17 `managed_send_storage_faults_never_dispatch_without_claim_or_retire_unreceipted_payload`: six fixtures with DB triggers
- rank 18 `public_dispatch_recovers_lost_requests_and_replies_without_duplicate_effects`: fault proxy frame loss
- rank 19 `public_pi_admission_uses_final_slot_and_allows_more_than_four_children`: 50-session matrix
- rank 20 `forced_cancel_kills_pi_process_group`: A 40s deadline, SIGSTOP and real process-group probes
- rank 21 `managed_creation_storage_failures_permit_only_durably_claimed_effects`: six fixtures with DB triggers
- rank 22 `public_dispatch_freezes_fresh_child_target_before_metrics_and_write`: acceptance-diagnostics feature
- rank 23 `claude_reopen_retains_exact_conversation_before_another_callback`: real restarts+sqlite
- rank 24 `questions::public_question_sixty_four_answer_effects_exhaust_storage_without_eviction_or_replay`: real managed PTY
- rank 25 `extension_conversations_switch_and_survive_repeated_restart`: real restarts
- rank 26 `managed_live_cancel_known_run_requires_durable_intent_and_authoritative_outcome`: four PTY sessions
- rank 27 `three_failed_title_attempts_close_window_and_survive_restart`: provider exits 2 per call
- rank 28 `ordinary_cloud_agent_validates_preview_and_cleans_owned_tree_on_exit_and_signals`: four PTY sessions
- rank 29 `fallback::draft_public_same_run_old_future_unrelated_and_truncated_markers_stay_incomplete`: real managed PTY
- rank 30 `observation::production_observer_deadline_reconnect_overflow_and_wait_abort_leave_child_running`: real managed PTY

No speedup number is claimed for either bucket: the durations are concurrent process time, and a removal would only remove the process, not a measured wall-clock span.

## 5. Retry and flake evidence

| test_id | first attempt | total | attempts | failed | attempt rows |
|---|---:|---:|---:|---:|---|
| `nextest\|Test\|ovrcr%3A%3Acodex_initial_prompt\|codex_harness_default_and_explicit_prompt_use_the_shared_lifecycle_and_privacy_path\|default` | 4.855 | 9.227 | 2 | 1 | attempt 1 FAIL 4.855 s; attempt 2 PASS 4.372 s |
| `nextest\|Test\|ovrcr%3A%3Api_delegation\|parent_recovery%3A%3Aparent_recovery_transfer_discovers_prepared_and_suspends_accepted_unknown_partial_effects\|default` | 24.507 | 46.250 | 2 | 1 | attempt 1 FAIL 24.507 s; attempt 2 PASS 21.743 s |

Two tests failed once and passed on the single automatic retry. A single hosted run establishes neither a flake rate nor a CI speedup; the retained total includes both attempt durations.

## 6. Python and Node timing gaps

**python-unittest** — 39 rows, 0 timed (`timing_basis=unmeasured`). Only module-level aggregates exist: InstallerTests 33 tests in 10.080 s (`macOS-job.log:3631`); the Cursor bridge checks 6 tests in 2.076 s (`macOS-job.log:3669`). unittest prints one aggregate per module; no per-method timing exists, so rows carry null durations.

**python-check** — 200 rows, 0 timed (`timing_basis=unmeasured`). Suite-level totals only: the relocated startup-assets step is 82 s; aggregate counts are 12 then 28 relocated payload checks (`:3634`, `:3644`), 27 then 36 packaging checks (`:3635`, `:3645`), 40 then 40 guard checks (`:3636`, `:3646`), and 17 recipe checks (`:3469`). script checks print aggregate counts only; no per-check duration exists, so rows carry null durations.

**node-test** — 110 rows all have per-case TAP durations summing to 9.531 s, but the batch header in `node-tap.txt` reports 4.243 s over 106 cases: Node runs files concurrently and the inventory keeps 110 rows for 106 cases, so the per-case sum is not additive and no per-suite wall attribution is possible.

**cargo-test** — 2 rows, both timed (5.370 s and 7.250 s) in separate installed-Pi steps (31 s and 16 s step durations).

## 7. Derivation, reconciliation and data quality

- `sum(attempts.csv.seconds)` = `sum(inventory.csv.total_attempt_seconds)` for the 2820 nextest rows = `sum(suite totals)` = **5931.968 s**; attempts rows 2822 = 2820 first attempts + 2 retries.
- Test step: 136 s compilation + 2014.918 s nextest execution + 3.082 s residual = 2154 s.
- Step-covered 2415 s (job-record 2418 s) includes setup, format/lint, the Test step, doc tests, package checks, extension tests and installed-Pi steps; it is not the sum of test-process durations.
- Data quality: reviews/other-rust-integration-006.jsonl: cost_drivers for forced_cancel_kills_pi_process_group is a string, not an array; recorded verbatim as the cost driver and used unchanged.
- Data quality: inventory.csv node-test rows sum to 9.531 s while the node-tap.txt batch header reports 4.243 s over 106 cases; per-case durations are retained as reported, batch wall time is reported separately.

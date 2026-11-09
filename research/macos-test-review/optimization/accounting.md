reviewer_model: openai-codex/gpt-6.1-sol

# Nextest wall-time accounting

## Utilization verdict

**With three test slots, this run is work-bound. With four slots, it has substantial unexplained scheduling/occupancy headroom; the known exclusive tests alone do not explain it.** “Work” here means elapsed test-process occupancy, including blocked I/O and sleeps. It does not mean CPU time. These data cannot establish CPU saturation or identify the runner's actual core count.

The measured baseline is one hosted run: [42429838269, job 128576086691](https://github.com/xlyk/ovrcr/actions/runs/42429838269/job/128576086691), revision `992a1da5c59a865ebb48051e4d9d8ff7184c8dc5`. All evidence paths below are relative to `research/macos-test-review/`; source/config paths are relative to the repository.

- `evidence/attempts.csv`: 2,822 attempts for 2,820 distinct tests, totaling **W = 5,931.968 process-seconds**. Group by `(invocation, binary_id, test_name)` before ranking tests. The two retries are included once per attempt.
- `evidence/cost-analysis.json`, `total_picture.nextest_execution_seconds`: **T = 2,014.918 wall-seconds**. `evidence/job-steps.csv`, row `Test`, reports 2,154 seconds, including 136 seconds of compilation. The remaining 3.082 seconds are step bookkeeping. Compilation is outside this ledger.
- Average occupied test slots: **W/T = 2.944024521**. This is the same observed concurrency under both hardware hypotheses, not “2.94 cores used.”
- `.config/nextest.toml:15-22` reserves every test slot for four exact tests. Their observed total is **E = 37.240 seconds**. The other tests account for **W − E = 5,894.728 process-seconds**.

Assume (only for the following model) one nextest slot per vCPU, fixed per-test durations, independent nonexclusive tests, no CPU/IO interference changes, and no unrecorded scheduling constraints. Ideal scheduling with the current exclusive tests has the lower bound `E + max((W−E)/p, L)`, where `p` is the slot count and `L = 68.639 seconds` is the longest nonexclusive test. The work term dominates `L` in both cases.

| Accounting quantity | Three slots / 3 vCPUs | Four slots / 4 vCPUs |
|---|---:|---:|
| Occupied-slot fraction, `W/(pT)` | 98.134% | 73.601% |
| Work-only theoretical floor, `W/p` | 1,977.323 s | 1,482.992 s |
| Floor preserving exclusivity, `E+(W−E)/p` | **2,002.149 s** | **1,510.922 s** |
| Extra floor cost from exclusivity, `E(1−1/p)` | 24.827 s | 27.930 s |
| Observed wall minus exclusive-aware floor | **12.769 s** | **503.996 s** |
| Unoccupied slot-seconds, `pT−W` | 112.786 | 2,127.704 |
| Of those, unavoidable while exclusive tests run, `(p−1)E` | 74.480 | 111.720 |
| Unoccupied slot-seconds left after exclusivity | 38.306 | 2,015.984 |

Thus the three-slot identity is `2,014.918 = 37.240 + 5,894.728/3 + 12.769` (rounded). The four-slot identity is `2,014.918 = 37.240 + 5,894.728/4 + 503.996`. The last term is an accounting remainder, not a measured scheduler defect. It can include unfilled slots, start/finish overhead, tail effects, and constraints absent from this model. The aggregate evidence does not resolve those components. If the actual slot count is three, adding a fourth vCPU without changing nextest's slot count does not create the four-slot floor.

The largest single test occupies only 3.407% of observed wall time. It cannot explain a 33-minute critical path by itself. The known exclusive interval is 1.848% of wall; its incremental cost over fully parallel slot use is the smaller 24.827–27.930-second floor penalty above. Do not count both the full exclusive duration and its incremental penalty as separate costs.

## Concentration

### Binary work

Source: `evidence/attempts.csv`, sum `seconds` by `binary_id`, joined to unique nextest rows in `tests.csv`. Counts below are tests, not attempts. `evidence/cost-analysis.json:suite_costs` provides an independent named summary; its `attempt_rows` is 260 rather than 259 for Pi because that binary retried one test.

| Binary | Tests | Process-seconds, all attempts | Share of W |
|---|---:|---:|---:|
| `ovrcr::pi_delegation` | 259 | 2,380.841 | 40.136% |
| `ovrcr::server_lifecycle` | 157 | 764.781 | 12.893% |
| `ovrcr::cursor_cloud` | 47 | 702.799 | 11.848% |
| `ovrcr::automatic_titles` | 34 | 307.713 | 5.187% |
| `ovrcr::retained_sessions` | 22 | 219.666 | 3.703% |
| `ovrcr::claude_quota_probe` | 14 | 167.450 | 2.823% |
| `ovrcr-runtime` | 496 | 154.868 | 2.611% |
| `ovrcr::terminal_acceptance` | 34 | 154.485 | 2.604% |
| `ovrcr::provider_quota` | 33 | 134.734 | 2.271% |
| `ovrcr::gui` | 13 | 99.716 | 1.681% |
| `ovrcr::retained_ownership` | 13 | 94.812 | 1.598% |
| `ovrcr::task_execution` | 13 | 87.274 | 1.471% |
| `ovrcr::cli` | 50 | 66.545 | 1.122% |
| `ovrcr::client_startup_server` | 21 | 61.644 | 1.039% |
| `ovrcr::retained_capacity` | 5 | 57.052 | 0.962% |
| Remaining 41 binaries | 1609 | 477.588 | 8.051% |

The top three binaries total **3,848.421 seconds (64.876%)**. The top fifteen total **5,454.380 seconds (91.949%)**. These are concurrent process-time shares, not additive job-wall shares.

### Test work

Source: `tests.csv`, `runner=nextest`, sorted by numeric `total_attempt_seconds` descending and `test_id` ascending. These totals match grouping `evidence/attempts.csv`; the top thirty also match `evidence/cost-analysis.json:slowest_tests`.

| Largest tests | Process-seconds | Share of W |
|---|---:|---:|
| Top 1 | 68.639 | 1.157% |
| Top 10 | 518.005 | 8.732% |
| Top 30 | 1,089.436 | 18.366% |
| Top 100 | 2,192.033 | 36.953% |
| Other 2,720 | 3,739.935 | 63.047% |

| Binary / test | Process-seconds | Share of W | Defining source |
|---|---:|---:|---|
| `ovrcr::cursor_cloud` / `managed_cloud_refuses_incompatible_bridge_before_account_requests` | 68.639 | 1.157% | `tests/cursor_cloud.rs:371-438` |
| `ovrcr::claude_quota_probe` / `claude_probe_startup_without_usage_reaches_the_deadline_and_the_process_is_gone` | 62.920 | 1.061% | `tests/claude_quota_probe.rs:356-358` |
| `ovrcr::claude_quota_probe` / `claude_probe_silent_for_sixty_seconds_times_out_and_the_process_is_gone` | 62.912 | 1.061% | `tests/claude_quota_probe.rs:351-353` |
| `ovrcr::cursor_cloud` / `managed_send_uncertainty_and_malformed_evidence_never_retry_or_become_ready` | 56.663 | 0.955% | `tests/cursor_cloud.rs:1705-1793` |
| `ovrcr::pi_delegation` / `dependencies::public_dependency_lost_requests_and_replies_reconcile_original_baseline_without_duplicate_effects` | 55.590 | 0.937% | `tests/pi_delegation/dependencies.rs:1241-1316` |
| `ovrcr::pi_delegation` / `parent_recovery::parent_recovery_transfer_discovers_prepared_and_suspends_accepted_unknown_partial_effects` | 46.250 | 0.780% | `tests/pi_delegation/recovery.rs:801-911` |
| `ovrcr::cursor_cloud` / `managed_send_crash_and_discard_boundaries_preserve_uncertainty_and_exact_late_identity` | 46.006 | 0.776% | `tests/cursor_cloud.rs:1940-2050` |
| `ovrcr::pi_delegation` / `followups::public_followup_storage_limit_preserves_last_result_acceptance_and_all_receipts` | 42.056 | 0.709% | `tests/pi_delegation/followups.rs:529-600` |
| `ovrcr::cursor_cloud` / `managed_creation_crashes_retain_original_operation_and_require_explicit_discard` | 39.866 | 0.672% | `tests/cursor_cloud.rs:609-768` |
| `ovrcr::cursor_cloud` / `managed_recovery_account_source_and_unknown_outcomes_fail_closed` | 37.103 | 0.625% | `tests/cursor_cloud.rs:3351-3473` |

The single slowest attempt is also the slowest test: `ovrcr::cursor_cloud::managed_cloud_refuses_incompatible_bridge_before_account_requests`, **68.639 seconds**, one attempt. It loops over thirteen bridge-version/capability cases, making and launching a fresh fixture for each (`tests/cursor_cloud.rs:371-438`). An indivisible test establishes a 68.639-second lower bound at any fixed thread count. With the exclusive interval, the bound is at least `37.240 + 68.639 = 105.879 seconds`, still far below either work floor. A late-starting long test can leave a tail, but this single run does not isolate that tail or show that case splitting would reduce wall time.

Two other long tests deliberately require at least sixty elapsed seconds and check that the provider process is gone (`tests/claude_quota_probe.rs:350-389`). Their measured 62.920 and 62.912 seconds are behavioral work, not automatically wasted setup.

## Cost by review and scheduling class

### Additive nextest ledger

Every nextest test appears in exactly one row here. The order separates confirmed duplicate executions from the retained shared-fixture instance. It then separates rejected proposals, retried tests, exclusive tests, and the remainder. The shared fixture's full **28 executions / 3.446 seconds** are the first two rows together.

Source: `tests.csv` identity, recommendation, source, and timing columns; `evidence/overlap-groups.json` member-minus-retained ID sets; `evidence/findings-inventory.json:proposal_rejected`; exact config filters in `.config/nextest.toml:15-22`.

| Disjoint class | Tests | Process-seconds | Share of W |
|---|---:|---:|---:|
| Shared-fixture consolidation candidates | 27 | 3.345 | 0.056% |
| Retained shared-fixture execution | 1 | 0.101 | 0.002% |
| Other timed consolidation candidates | 15 | 0.999 | 0.017% |
| Rejected consolidation proposals, retained | 9 | 5.995 | 0.101% |
| Tests that retried, both attempts included | 2 | 55.477 | 0.935% |
| Exclusive tests | 4 | 37.240 | 0.628% |
| All other nextest tests | 2,762 | 5,828.811 | 98.261% |
| **Total** | **2,820** | **5,931.968** | **100%** |

There are **54 consolidation candidates**, not 54 timed nextest removals: **42 nextest candidates / 4.344 measured seconds**, plus **12 Python logical checks with unknown per-check durations**. Those twelve run outside nextest. The 28 overlap groups contain 54 unique candidate IDs after subtracting each group's retained IDs. The count and sum use each candidate's own row once; no retained duration is included. `tests.csv:overlap_test_ids` supplies comparison links, not extra seconds. Retained IDs may be shared across groups, so summing group member durations would be wrong.

The repeated body builds a private fixture and checks three environment removals (`tests/support/live.rs:111-145,163-195,567-579`). It does not launch the constructed OVRCR command in this body. The fixture does initialize its repository. The CSV's “single spawn” wording is therefore a coarse mechanics annotation, not evidence that the three assertions themselves launch a server. The timings above come from numeric timing columns, never numbers embedded in `cost_drivers` text.

### Rejected proposals

Each row below is `keep` in `tests.csv`. The inventory's rejection explanations preserve distinct protection; their duration is not a deletion budget. Source: `evidence/findings-inventory.json:proposal_rejected`, joined by full `test_id` to `tests.csv`.

| Binary / test | Process-seconds |
|---|---:|
| `ovrcr::server_lifecycle` / `agent_admission_later_patch_preserves_resume_identity_and_lifecycle` | 4.802 |
| `ovrcr::tui` / `agent_search::control_hold_labels_visible_running_agents_without_moving_the_title_column` | 0.041 |
| `ovrcr::tui` / `agent_search::long_visible_agent_list_uses_first_nine_single_digit_labels_and_search_stays_available` | 0.070 |
| `ovrcr-runtime` / `server::quota::tests::protocol_failure_not_version_is_what_reports_unsupported` | 1.016 |
| `ovrcr-tui` / `dashboard::cards::tests::a_narrow_unread_row_keeps_its_whole_state_and_age` | 0.012 |
| `ovrcr-tui` / `dashboard::desktop::tests::desktop_input_request_in_a_visible_pane_alerts` | 0.012 |
| `ovrcr-tui` / `dashboard::desktop::tests::desktop_provider_session_event_reaches_notification_observer` | 0.012 |
| `ovrcr-runtime` / `settings::tests::quota_in_instance_identity_is_a_finding_and_configures_nothing` | 0.011 |
| `ovrcr::task_tui` / `destructive_actions_require_confirmation_and_escape_cancels` | 0.019 |
| **Total** | **5.995** |

### Retries

Source: `evidence/attempts.csv:342,345,717,720`; `tests.csv` has the same first/total durations and attempt counts. Each failed once and passed once.

| Binary / test | Failed first attempt | Successful retry | Total |
|---|---:|---:|---:|
| `ovrcr::codex_initial_prompt` / `codex_harness_default_and_explicit_prompt_use_the_shared_lifecycle_and_privacy_path` | 4.855 s | 4.372 s | 9.227 s |
| `ovrcr::pi_delegation` / `parent_recovery::parent_recovery_transfer_discovers_prepared_and_suspends_accepted_unknown_partial_effects` | 24.507 s | 21.743 s | 46.250 s |
| **Total** | **29.362 s** | **26.115 s** | **55.477 s** |

The measured rerun work is 26.115 seconds. A counterfactual clean pass could instead resemble the 29.362 seconds of failed-attempt work; neither is an observed clean-run baseline. Treat 26.115–29.362 process-seconds only as a scenario range for one redundant attempt, conditional on unchanged attempt durations. This sample does not estimate a flake rate, and turning off retries would not preserve the current success behavior.

### Exclusive tests

All four belong to `ovrcr::server_lifecycle`. Source: exact-name filters `.config/nextest.toml:15-22`, joined to `tests.csv` and `evidence/attempts.csv`. Other similarly named `desktop_notifications_bridge_click_*` tests are **not** exclusive under this config.

| Exact test name | Observed duration | Defining source |
|---|---:|---|
| `desktop_notifications_bridge_click_focuses_existing_split_without_duplicating_target` | 6.902 | `tests/server_lifecycle.rs:13125-13163` |
| `desktop_notifications_bridge_click_discards_transient_drafts_and_releases_capture` | 10.796 | `tests/server_lifecycle.rs:13167-13247` |
| `desktop_notifications_bridge_click_lands_in_browse_without_reviewing_or_answering` | 9.960 | `tests/server_lifecycle.rs:13046-13121` |
| `iterm_setup_real_palette_requires_separate_opt_in_and_uses_current_owned_dashboard` | 9.582 | `tests/server_lifecycle.rs:20377-20537` |
| **Total** | **37.240 s** | |

They add **37.240 seconds of serial wall occupancy**, during which no other nextest test can run. Compared with spreading the same work over all slots, the idealized incremental floor cost is **24.827 seconds at three slots** or **27.930 seconds at four slots**. Their own durations already occur in W. The exclusivity is a race-protection requirement, not automatically safe to remove.

### Mechanics indicators without double counting

The setup/work partition below assigns each row to its first matching class. For readers who need the inclusive census, these flags overlap and must **not** be added:

| Positive indicator in `cost_drivers` | Nextest tests | Their whole-test process-seconds |
|---|---:|---:|
| Fixed wait/settle wording, narrow rule below | 27 | 147.064 |
| Any sleep/settle wording after removing `no sleeps` | 52 | 264.782 |
| Polling | 235 | 1,540.562 |
| PTY | 376 | 3,347.633 |
| Process/spawn/lifecycle wording after exclusions | 344 | 960.670 |
| Union of any sleep/settle, polling, PTY, process indicators | 734 | 4,189.860 |

A `sleep 30` child killed promptly is not thirty seconds of fixed waiting. Poll-loop sleeps are also not fixed settling delays. The narrow fixed-wait rule separates those cases; the broader sleep row is supplied so that neither meaning disappears. All seconds are the tests' measured elapsed durations, not time spent inside that mechanism.

The inclusive union counts each full test ID once. These pairwise intersections explain why adding indicator totals would overcount; intersections themselves also overlap and must not be added:

| Intersecting indicators | Nextest tests | Whole-test process-seconds |
|---|---:|---:|
| Sleep/settle and polling | 15 | 101.918 |
| Sleep/settle and PTY | 7 | 79.117 |
| Sleep/settle and process/spawn | 19 | 72.003 |
| Polling and PTY | 143 | 1,264.868 |
| Polling and process/spawn | 81 | 455.988 |
| PTY and process/spawn | 69 | 416.197 |

## Setup versus assertion work

**The supplied evidence cannot measure how many tests, or how many seconds, are dominated by setup rather than assertions.** It has whole-process elapsed durations and prose cost annotations, not phase timings or profiles. Calling all 3,347.633 PTY-associated seconds “PTY startup overhead,” for example, would silently count behavioral waits and assertions as setup. The sixty-second deadline checks above demonstrate the problem.

The reproducible proxy is **tests with positive setup, fixture, lifecycle, or wait annotations**. It covers **1,603 nextest tests / 5,789.449 process-seconds (97.597% of W)**, and **1,845 rows across all runners**. Those are whole-test upper envelopes of affected work, not measured avoidable setup. The remaining **1,217 nextest tests / 142.519 seconds** lack the selected positive annotations; that does not prove they are assertion-only.

For the following partition, inspect `cost_drivers` for all 3,171 `tests.csv` rows, case-insensitively, in the listed order. Remove negative or irrelevant process phrases before matching: `no sockets or child processes`; `no (child |worker )?process(es)?( spawned| spawn expected)?`; `no filesystem or processes`; `no filesystem or process`; `no sleeps`; `one nextest process`; `in-process`; `process[- ](local|global)`; `pure process-model algorithm`; `process home`. This avoids counting “one nextest process; no I/O” as fixture spawning. These are textual rules, not a second source-level cost review.

| First-match class | Matching expression after exclusions |
|---|---|
| Fixed wait/settle | `\bsettle\b\|backoff sleep\|260ms sleep\|sleep\(100ms\)\|1s internal sleep\|2s version sleep\|provider sleeps 2 s\|real sleeps` |
| Polling | `\bpoll(?:s\|ing)?\b` |
| PTY | `\bptys?\b\|pseudo[- ]?terminal` |
| Process/spawn | `\bspawn(?:s\|ing\|ed)?\b\|subprocess(?:es)?\|\bprocess(?:es)?\b\|\bfork\b\|\blaunch(?:er\|es\|ing\|ed)?\b` |
| Storage/Git/filesystem | `sqlite\|database\|\bdb\b\|filesystem\|\bfs\b\|\bfiles?\b\|pidfile\|tempdir\|temp \|worktree\|workspace\|\bgit\b\|directory\|directories\|clone\|hardlink\|symlink\|registry\|persistence\|retained store` |
| Socket/network/thread | `socket\|unixstream\|\btcp\b\|\bhttp\b\|thread\|channel` |
| Other fixture/lifecycle/time | `fixture\|mock\|fake\|script\|provider\|server\|dashboard\|restart\|binary\|compiled\|\bcli\b\|host\|\bsessions?\b\|stream\|\bgate\b\|signal\|sigkill\|sigterm\|shutdown\|deadline\|timeout\|timing\|elapsed\|wall.clock\|budget\|\bwait\|backoff\|\bloop\|fault\|lock` |
| No selected positive annotation | No earlier match |

The fixed-wait match uses the original text; the other broad mechanics matches use the cleaned text. Polling and PTY exclusions do not change their matches. The escaped pipes in the Markdown source represent regex alternation, not literal pipes.

| Disjoint first-match class | All rows | Nextest tests | Nextest seconds | Other timed top-level rows / seconds | Nested rows, excluded from sums | Unknown-time rows |
|---|---:|---:|---:|---:|---:|---:|
| Fixed wait/settle annotation | 27 | 27 | 147.064 | 0 / 0.000 | 0 | 0 |
| Polling annotation | 235 | 234 | 1,533.663 | 1 / 7.250 | 0 | 0 |
| PTY annotation | 233 | 233 | 2,082.765 | 0 / 0.000 | 0 | 0 |
| Positive process/spawn annotation | 417 | 235 | 411.319 | 0 / 0.000 | 0 | 182 |
| Storage/Git/filesystem annotation | 383 | 359 | 670.752 | 0 / 0.000 | 0 | 24 |
| Socket/network/thread annotation | 135 | 135 | 125.327 | 0 / 0.000 | 0 | 0 |
| Other fixture/lifecycle/time annotation | 415 | 380 | 818.559 | 0 / 0.000 | 4 | 31 |
| No selected positive mechanics annotation | 1326 | 1217 | 142.519 | 107 / 13.060 | 0 | 2 |
| **Total** | **3,171** | **2,820** | **5,931.968** | **108 / 20.310** | **4** | **239** |

The other timed top-level rows are 106 Node tests plus two explicit Cargo test parents. The four installed-Pi Node children already contribute to their parents and are excluded from sums. The 239 unknown-time rows are 200 Python logical checks and 39 Python unittest methods. Unknown seconds remain unknown, not zero. No Python suite duration is divided among checks. The “0 / 0.000” cells mean an empty class of additional timed rows, not measured zero-duration Python tests. Extra-runner seconds are outside the nextest wall and excluded from every nextest utilization/headroom calculation.

## Headroom by class

These tables name cost classes only; they do not recommend deleting, disabling, or weakening tests. Difficulty is **[INFERENCE]** about coverage-preserving changes, not measured effort. No work represented by a full-test duration is presumed removable.

For nonexclusive work, the three-slot column is `S/3` and the four-slot column is `S/4`; percentages divide those figures by **2,014.918**. This is the idealized work-floor contribution if all S process-seconds vanished while other durations and scheduling stayed fixed. It is an upper envelope for examining a class, **not a speedup prediction**. Actual savings require the removable fraction, position on the schedule, and interference effects to be measured. These columns sum to `W/p`, not measured wall time; exclusivity and the accounting remainder explain the difference.

### Disjoint mechanics envelopes

These rows partition W exactly as in the preceding table. Exclusive and retried tests remain inside their first matching mechanics class here; do not add the next panel to this one.

| Class | Process-seconds at stake, S | Three-slot floor contribution / wall fraction | Four-slot floor contribution / wall fraction | Coverage-preserving difficulty [INFERENCE] |
|---|---:|---:|---:|---|
| Fixed wait/settle annotation | 147.064 | 49.021 / 2.433% | 36.766 / 1.825% | Medium–expensive; timing/race contract must survive |
| Polling annotation | 1,533.663 | 511.221 / 25.372% | 383.416 / 19.029% | Expensive; synchronization and fault ordering matter |
| PTY annotation | 2,082.765 | 694.255 / 34.456% | 520.691 / 25.842% | Expensive; real terminal/process boundary is the coverage |
| Positive process/spawn annotation | 411.319 | 137.106 / 6.805% | 102.830 / 5.103% | Medium–expensive; isolation and process cleanup matter |
| Storage/Git/filesystem annotation | 670.752 | 223.584 / 11.096% | 167.688 / 8.322% | Medium–expensive; durable-state and Git semantics matter |
| Socket/network/thread annotation | 125.327 | 41.776 / 2.073% | 31.332 / 1.555% | Medium; IPC/thread behavior may be the subject |
| Other fixture/lifecycle/time annotation | 818.559 | 272.853 / 13.542% | 204.640 / 10.156% | Expensive/unknown; mixed mechanisms need separation |
| No selected positive mechanics annotation | 142.519 | 47.506 / 2.358% | 35.630 / 1.768% | Unknown; no positive mechanics tag is not proof of assertion cost |

An alternative descriptive conversion holds the observed average concurrency fixed at `W/T = 2.944024521`. It maps process work S to `S/(W/T)` wall-equivalent seconds, or `S/W` of observed wall. This proportional conversion does not model the exclusive intervals separately and is not a prediction of marginal savings. The three- and four-slot floors above remain the scheduling bounds.

| Same disjoint mechanics class | Observed-concurrency wall equivalent | Fraction of observed wall |
|---|---:|---:|
| Fixed wait/settle | 49.953 s | 2.479% |
| Polling | 520.941 s | 25.854% |
| PTY | 707.455 s | 35.111% |
| Process/spawn | 139.713 s | 6.934% |
| Storage/Git/filesystem | 227.835 s | 11.307% |
| Socket/network/thread | 42.570 s | 2.113% |
| Other fixture/lifecycle/time | 278.041 s | 13.799% |
| No selected positive annotation | 48.410 s | 2.403% |

### Review, retry, and scheduling views of the same work

The rows here overlap both one another and the mechanics table. They are separate views, never an additive savings portfolio.

| Class | Seconds at stake | Three-slot model / fraction of T | Four-slot model / fraction of T | Coverage-preserving difficulty [INFERENCE] |
|---|---:|---:|---:|---|
| Confirmed timed consolidation candidates | 4.344 process-s; 12 additional Python checks unmeasured | 1.448 s / 0.072% | 1.086 s / 0.054% | Cheap–medium for proven repeated bodies; no nextest timing claim for Python |
| Shared body, all 28 executions | 3.446 process-s; only 3.345 are candidate copies | 1.149 s / 0.057% for all copies | 0.862 s / 0.043% for all copies | Cheap–medium, but retained protection is required; full envelope is not removable |
| Rejected proposals | 5.995 process-s | 1.998 s / 0.099% envelope | 1.499 s / 0.074% envelope | Expensive or unsupported: unique coverage prevents treating these as duplicate savings |
| Retry attempts actually executed | 26.115 process-s | 8.705 s / 0.432% | 6.529 s / 0.324% | Unknown; root cause not established by one failure each |
| Exclusive scheduling penalty, work retained | 37.240 serial-s; incremental penalty shown at right | **24.827 s / 1.232%** | **27.930 s / 1.386%** | Expensive: race-sensitive tests require isolation evidence |
| Unexplained remainder after work/exclusivity floor | Model residual, not measured wasted work | **12.769 s / 0.634%** | **503.996 s / 25.013%** | Unknown; actual runner slot count and schedule are missing prerequisites |

The broad class with material work is multi-component fixture/lifecycle/OS behavior, not the 54 consolidation candidates. At three slots, most wall time follows directly from elapsed test work; at four, scheduling/occupancy must first be distinguished from core count. A future measurement must record actual nextest slot count, per-attempt start/end intervals, and phase timing (fixture creation, behavioral wait, assertion, cleanup). Without those observations, setup dominance and a precise coverage-preserving speedup remain unmeasured.

## Evidence boundaries

This report performs only data analysis of the frozen CSV/JSON and source reads. It ran no tests, builds, formatters, native check scripts, or fixture programs and changed no product/test/configuration files. All duration sums use `Decimal` on the timing columns; display values round to three decimals. Table shares use the unrounded sums. Group and annotation counts use unique full IDs.

`evidence/cost-analysis.json` calls its 2,415-second step-covered span “job_wall_seconds”; that field derives the first/last step timestamps, not necessarily the job API's full wall interval. Neither that span nor the full job duration is used as the 2,014.918-second nextest denominator. A single hosted run supplies no timing distribution and no guaranteed speedup.

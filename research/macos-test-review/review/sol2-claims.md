reviewer_model: openai-codex/gpt-6.1-sol

verdict: SOUND

The final methodology's CSV totals, scores, timing arithmetic, overlap totals, receipt requirements, rejected-proposal table, and CSV fingerprint check out. The two remaining documentation issues from the preceding snapshot have been corrected. This report was refreshed after each material evidence rewrite; its earlier snapshots are superseded.

## Method

I read the current methodology and recomputed its claims with Python standard-library CSV/JSON parsing, Decimal sums, counters, medians, timestamps, SHA-256, and an in-memory CSV round-trip. I read the relevant assembler branches and production/test source. I ran the explicitly requested Git inspections. I did not run the assembler, product tests, builds, formatters, or Python/Node fixtures. Evidence paths below are relative to `research/macos-test-review/` unless they begin with `src/`, `tests/`, or `crates/`.

Reviewed snapshot: methodology SHA-256 `e6a4b2527249a8268b62985de4c847793e9921fe6642763a76f181415c1a0cf2` (reader snapshot `C62D`), CSV SHA-256 `dcccdb0ec9d19a3569a05285f33f7620e71308f3765edf2d0086996ca767fb5a`, 5,541,668 bytes. The initially assigned 29-group/55-consolidation totals and intermediate `763e111b…` output describe older files, not this snapshot.

## Checklist

1. **CONFIRMED — Inventory size and columns.** `tests.csv` has 3,171 data rows, 3,171 distinct IDs, and 40 columns. `evidence/inventory.csv` also has 3,171 rows. The CSV's ID set equals the inventory's ID set. These support sections 7, 8, and 11.

2. **CONFIRMED — Runner counts.** Counting `tests.csv.runner` gives nextest 2,820; cargo-test 2; node-test 110; python-unittest 39; python-check 200. The inventory agrees. The nextest population is separate from the two explicit Cargo invocations.

3. **CONFIRMED — Test levels.** Counting `test_level` gives unit 1,336; integration 886; acceptance 949. Their sum is 3,171.

4. **CONFIRMED — Recommendations.** Counting `recommendation` gives keep 3,109; consolidate 54; improve 8; prune_candidate 0; investigate 0. Sections 7, 8, 9, 10, and 11 agree on these current values.

5. **CONFIRMED — Other section 8 distributions.** Target kinds are integration 1,475; lib 1,269; script-check 200; node 110; bin 78; unittest 39. Variants are default 2,982; local-development 104; production 85. Both tables recompute directly from `tests.csv`.

6. **CONFIRMED — Score distributions.** Integer parsing and `statistics.median` over all CSV rows give criticality min/median/max 25/75/100, usefulness 25/80/100, and retention 31/75/100. Sections 7 and 8 agree. All 3,171 stored derived scores equal the formulas recomputed from C/S/U/F.

7. **CONFIRMED — Correctly separated job boundaries.** The 23 rows of `evidence/job-steps.csv` sum to 2,415 seconds. The first start is `00:27:42Z`; the last completion is `01:07:57Z`, also an interval of 2,415 seconds, or 40m15s. The `macOS checks` job in `evidence/run.json` spans `2026-11-08T00:27:42Z` to `2026-11-08T01:08:00Z`: 2,418 seconds, or 40m18s. The difference is 3 seconds. The section 6 table, section 6.3 caveat, and section 11 limitation now distinguish these correctly; item 29 identifies the leftover source-table label.

8. **CONFIRMED — Test-step breakdown.** `evidence/job-steps.csv:11` spans `00:28:43Z`–`01:04:37Z`, or 2,154 seconds. `evidence/macos-job.log:502` reports compilation in 2m16s (136 seconds); line 3396 reports nextest execution of 2,014.918 seconds. The residual is exactly 3.082 seconds at the displayed precision.

9. **CONFIRMED — Attempt totals.** `evidence/attempts.csv` has 2,822 attempt records, 2,820 distinct `(binary_id, test_name)` pairs, and 56 binary IDs. Summing its `seconds` with Decimal gives 5,931.968 seconds. The raw hosted summary at `evidence/macos-job.log:3396-3398` agrees on 2,820 tests and two recovered retries.

10. **CONFIRMED — Binary-group tables.** Grouping attempt rows by `binary_id` reproduces every displayed section 6/10 group:

    | Binary | Unique instances | Attempt rows | Seconds | Share |
    |---|---:|---:|---:|---:|
    | `ovrcr::pi_delegation` | 259 | 260 | 2380.841 | 40.14% |
    | `ovrcr::server_lifecycle` | 157 | 157 | 764.781 | 12.89% |
    | `ovrcr::cursor_cloud` | 47 | 47 | 702.799 | 11.85% |
    | `ovrcr::automatic_titles` | 34 | 34 | 307.713 | 5.19% |
    | `ovrcr::retained_sessions` | 22 | 22 | 219.666 | 3.70% |
    | `ovrcr::claude_quota_probe` | 14 | 14 | 167.450 | 2.82% |
    | `ovrcr-runtime` | 496 | 496 | 154.868 | 2.61% |
    | `ovrcr::terminal_acceptance` | 34 | 34 | 154.485 | 2.60% |
    | `ovrcr::provider_quota` | 33 | 33 | 134.734 | 2.27% |
    | `ovrcr::gui` | 13 | 13 | 99.716 | 1.68% |

    The rewritten description of pi_delegation as 259 unique instances over 260 attempt rows is correct.

11. **CONFIRMED — Top-binary shares.** Sorting all binary Decimal totals gives top-three 3,848.421 seconds / 5,931.968 = 64.88% rounded to two decimals; top-fifteen 5,454.380 seconds = 91.95%. These match section 10 and `evidence/cost-analysis.json`.

12. **CONFIRMED — Slowest 30.** Sorting measured, parentless CSV rows by `total_attempt_seconds` descending gives a 30-row sum of 1,089.436 seconds, or 18.37% of nextest cumulative process time. Exactly 29 have C≥3. The sole C2 row is `desktop_notifications_bridge_expired_permission_check_recovers_without_reauthorization`. All 30 recommendations are `keep`. These values agree with `evidence/cost-analysis.json`.

13. **CONFIRMED — Both retry records.** Grouping `evidence/attempts.csv` by binary and name gives only these repeated executions:

    | Test | First result/time | Second result/time | Total | Attempts / failed |
    |---|---|---|---:|---:|
    | `ovrcr::codex_initial_prompt::codex_harness_default_and_explicit_prompt_use_the_shared_lifecycle_and_privacy_path` | FAIL / 4.855s | PASS / 4.372s | 9.227s | 2 / 1 |
    | `ovrcr::pi_delegation::parent_recovery::parent_recovery_transfer_discovers_prepared_and_suspends_accepted_unknown_partial_effects` | FAIL / 24.507s | PASS / 21.743s | 46.250s | 2 / 1 |

    Their CSV first/total/count/failed fields match. `evidence/macos-job.log:3397-3398` labels both FLAKY. No final-summary row was added to the attempt ledger.

14. **CONFIRMED — Parent/nested timing numbers.** `evidence/macos-job.log:4428,4481` reports the explicit Cargo parents at 5.37s and 7.25s. Lines 4415 and 4476 report nested TAP stream totals of 1492.879958ms and 1904.338042ms. These are contained within their parent execution; the document correctly warns against adding them to parent durations.

15. **CONFIRMED — Overlap totals and group examples.** `evidence/overlap-groups.json` now has 28 objects, 85 member references, and 81 unique member IDs. `og-001` has 28 members and retains the stated automatic_titles ID, which is the lexicographically smallest member. The other 27 groups comprise 24 two-member and 3 three-member groups. `og-008` has 3 members; `og-019` has 2. These match the rewritten section 9 and source table.

16. **CONFIRMED — Consolidation count and high-criticality subset.** Filtering the current CSV gives 54 consolidate rows, **7 with C≥3**, matching the corrected sentence at `methodology.md:481` and source-table claim at line 606. Each current consolidation has a second reviewer and group/protector reference; `evidence/validation.json` check 6 records 28 structurally valid groups and zero unprotected removals. The withdrawn quota row remains keep/U=2 and `og-012` remains absent.

17. **CONFIRMED — Eight improve rows and score-floor arithmetic.** Filtering the CSV yields exactly the eight names in section 10, including the workflow-tracker lost-create-reply case. Exactly one row has usefulness 25: `dashboard::desktop::tests::desktop_confirmed_without_unread_does_not_queue`, C2/S1/U0/F3, consolidate. The next distinct usefulness is 30; the minimum usefulness among C≥3 rows is 35. There are zero prune candidates.

18. **CONFIRMED — Nine rejected proposals and complete table.** `methodology.md:490` now says nine, its table at lines 494–502 has nine rows, and `evidence/findings-inventory.json.proposal_rejected` has nine corresponding entries. All nine inventory IDs name current keep rows. The added quota and destructive-action withdrawals are both present. Source confirms their distinctions: `crates/ovrcr-runtime/src/settings/mod.rs:1536-1544` checks effective `report.settings.quota`, whereas `crates/ovrcr-runtime/tests/settings_matrix.rs:778-803` compares `report.rows`; `tests/task_tui.rs:231-239` cancels and confirms on one view, while `tests/task_tui.rs:471-536` creates a fresh view per answer. The findings inventory also has 54 confirmed proposals, so 54+9=63 original proposals. The version numerals 2.1.274 and 2.1.266 are confirmed by `tests/server_lifecycle.rs:9956-9965`. The `[1]`, `[01]`, and five-column restoration details are confirmed by `tests/tui/agent_search.rs:145-161` and `tests/tui/session_jump_single_digit.rs:338-349`.

19. **CONFIRMED — Review population and effective receipt requirement.** The CSV has 162 C4 rows and zero low-confidence rows. The 17 segmented `quality-review-*.jsonl` files contain 688 records over 687 distinct IDs; the canonical `evidence/quality-review.jsonl` has 687 distinct records. Recomputing the union of C4, removal recommendation, and low-confidence IDs from the current CSV gives **213 required IDs**, all with receipts. Sections 8 and 11 now agree on 213/213; `evidence/validation.json:542-545` records `receipts_required=213`.

20. **CONFIRMED — Receipt source attribution now resolves to current requirements.** `evidence/missing-receipts.json` now has `require_receipt_total=213`, `satisfied=213`, `missing=0`, and 213 requirement-list entries. Its requirement ID set exactly equals the current CSV gate union. These values agree with `evidence/validation.json:542-545` and the methodology's source-table citation at line 603. The old 215 is explicitly retained as `prior_require_receipt_total`, not the current requirement. The artifact's `generated_from.effective_consolidate=56` remains historical author-input metadata; the current output count is established by the CSV and validation receipt as 54, not by that field.

21. **CONFIRMED — Section 11 verification numbers.** The current `evidence/validation.json.checks` contains nine passed checks. Its counts agree with independent calculations: 3,171 rows/IDs, 3,171 reviews, 22,197 rationale cells, no placeholder or under-12-character required rationale, 3,171 existing/in-bounds defining ranges over 250 distinct definition files, 54 consolidations, 162 C4 rows, no low-confidence rows, 213 required receipts, 28 groups, and the attempt totals above. The receipt's broader source-validation file count is 320 because it also checks production references; that does not contradict the 250 definition-file count. An independent in-memory CSV write/read preserved all 3,171×40 cells. I inspected the existing nine-check receipt rather than rerunning the assembler.

22. **CONFIRMED — Current CSV fingerprint.** Re-hashing the latest file gives `dcccdb0ec9d19a3569a05285f33f7620e71308f3765edf2d0086996ca767fb5a`; its size is 5,541,668 bytes. Both exactly match the final sentence at `methodology.md:554` and the newly read `evidence/validation.json.csv` (`rows=3171`, same hash and byte count). The initially reviewed `5b1a9803…` and intermediate `763e111b…` artifacts are no longer the current output.

23. **CONFIRMED — Gates are correctly presented as review policy, not fully automated semantic enforcement.** Section 5's ordered policy matches `evidence/rubric.md:47-55`. It is not identical to everything the validator accepts or rejects. The new sentence at `methodology.md:355` is accurate: the validator does not require `confidence == "high"` for consolidate/prune_candidate, and it does not establish rationale truth from source. It checks confidence membership in the allowed enum, numerical prune thresholds, overlap/group structure, surviving protection, and independent receipt structure. It does not implement the full first-match semantic policy.

    There is also a stricter structural condition: the validator requires a prune candidate to have overlap IDs in a confirmed group. The written policy's alternative of no effective behavioral protection does not, by itself, satisfy the implementation. This distinction matters for hypothetical future prune rows; the present output has none. The new structural-versus-review explanation should not be read as saying that all written policy cases are accepted by the validator.

    Exact branches from `evidence/assemble.py:940-966`:

    ```python
        if recommendation == "prune_candidate":
            prune_rows += 1
            if not is_int(criticality) or criticality > 2:
                offenders.append(f"{tid}:prune_candidate with criticality={criticality!r} (requires C<=2)")
            if usefulness is None or usefulness > 25:
                offenders.append(
                    f"{tid}:prune_candidate with usefulness_score={usefulness!r} (requires <=25)"
                )
            if not overlap_list:
                offenders.append(f"{tid}:prune_candidate without overlap_test_ids")
            elif groups_ok:
                qualifying = False
                for group in member_index.get(tid, []):
                    members = set(_group_members(group) or [])
                    if tid in members and set(overlap_list) <= members:
                        qualifying = True
                        break
                if not qualifying:
                    offenders.append(
                        f"{tid}:prune_candidate overlap ids not co-membered in a confirmed group"
                    )
        elif recommendation == "consolidate":
            consolidate_rows += 1
            if not overlap_list:
                offenders.append(f"{tid}:consolidate without overlap_test_ids")
            elif groups_ok and not member_index.get(tid):
                offenders.append(f"{tid}:consolidate without a confirmed group containing the row")
    ```

    `assemble.py:1007-1019` computes required independent reviews; `1031-1161` checks group integrity and surviving protectors. No high-confidence condition appears in those branches.

24. **CONFIRMED — Reading guide and value/cost separation.** Actual CSV keys `(int(retention_score), test_id)` are ascending. `evidence/assemble.py:1546,1621-1634` writes and checks that ordering, so “weakest first” is accurate as a value-score reading order, not an instruction to prune. The three definitions at `methodology.md:445-453` agree with `evidence/rubric.md:5-34` and `assemble.py:641-653`: criticality measures consequence, usefulness combines S/U/F, and retention combines C/S/U/F. All formulas have positive dimension weights and no runtime/retry/cost term. The rubric explicitly forbids lowering value for runtime, retries, or maintenance friction.

25. **CONFIRMED — Tracked-tree limitation.** The requested `git status --porcelain` returned only `?? .omp/` and `?? research/macos-test-review/`. `git diff --stat` returned no entries. `git rev-parse HEAD` returned `992a1da5c59a865ebb48051e4d9d8ff7184c8dc5`. These observations support the current claim that tracked tests, assertions, and CI selection remain unchanged. They do not depend on the report's prose.

26. **CONFIRMED — Timing gaps and single-run caveat.** All 39 python-unittest rows and all 200 python-check rows have `timing_basis=unmeasured` and empty first/total/attempt/failed fields: 239 unmeasured rows, not zero-duration rows. `evidence/macos-job.log:3631,3669` reports unittest aggregates of 10.080s and 2.076s. Lines 4338-4346 report 106 Node cases and 4242.999792ms, correctly rounded to 4.243s. Sections 6 and 11 explicitly state that one hosted run establishes neither a flake rate nor a CI speedup and that concurrent/nested durations cannot be subtracted from wall time.

27. **CONFIRMED — One product observation, two inventory entries.** `evidence/findings-inventory.json.product_observation` contains two entries citing the same `src/remote/mod.rs:703-710` prefix check, attached to adjacent remote tests. They describe one concern, not two. The code returns true immediately for any string beginning `[::1]`; `src/remote/mod.rs:144-165` consequently skips login checks for that Host value. A GET request with `Host: [::1].example` therefore follows the stated authorization path without login. This is a source-derived observation, not a live exploit test. The source also splits non-prefix hosts at the first colon. The rewritten one-observation/two-entry count is accurate.

28. **CONFIRMED — Shared-fixture span.** `tests/support/live.rs:567` is the test attribute, line 568 starts `command_clears_inherited_reporting_authority`, and line 579 closes it. The 28 inventory rows all use defining range 568–579. `og-001` contains exactly these 28 execution IDs. The revised prose's attribute-inclusive span 567–579 and body span 568–579 are accurate; the broader 567–581 source-index citation merely includes the next impl opener.

29. **CONFIRMED — All job-boundary labels now agree.** The source-table claim at `methodology.md:595` now reads “Step-covered 2415 s (job record 2418 s); Test 2154 s; Doc tests 49 s; packaging/Node/Pi step durations.” Section 6.3 likewise says “2415 s step-covered interval (job-record wall time 2418 s).” Both match the timestamps and sums in item 7, `evidence/run.json`, and `evidence/job-steps.csv:2-24`. The old job-wall label is no longer a current finding.

30. **CONFIRMED — Retained state width is now corrected.** `methodology.md:498` now calls `Working · 2m` a **12-cell** state, matching `crates/ovrcr-tui/src/dashboard/cards.rs:1089` and the rendered-line width helper at `cards.rs:667-669`. `Working` contributes 7 cells, spaces and middle dot contribute 3, and `2m` contributes 2. The unread text at `cards.rs:1127-1130` is correctly described as 25 cells at width 40. The supporting rejection record's old 11-cell wording does not supersede the corrected source-grounded count.

## Conclusion

The final CSV and validation receipt agree with 28 overlap groups, 54 consolidations (7 with C≥3), 213 required reviews, and the `dcccdb0e…` fingerprint. The methodology now consistently reports nine rejected proposals and lists all nine. Its current receipt requirement is supported by the regenerated receipt artifact. No unresolved numerical contradiction or unsupported current claim remains in the assigned checklist. The distinction between written semantic review policy and the validator's narrower structural enforcement remains explicit in item 23.

Counts: **30 CONFIRMED / 0 WRONG / 0 UNVERIFIABLE** (30 numbered checklist items).

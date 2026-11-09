reviewer_model: openai-codex/gpt-6.1-sol; thinking level: unknown

# Verdict: MATERIAL-ERRORS

The recommendation policy and the validator's implemented gates differ. Four further errors concern the job-time label, the Pi binary's test-instance count, the number of distinct product observations, and the shared fixture's source span. The numerical distributions and cost sums below reproduce from the frozen CSV and the evidence.

Model provenance: the runtime's project context exposes `openai-codex/gpt-6.1-sol`. It exposes no thinking level; the process environment contained no model/thinking/reasoning variables. No thinking level is inferred.

## Scope and method

All CSV-content checks use `review/frozen/tests.csv`, not the live CSV. Paths below are relative to `research/macos-test-review/` unless they name repository source. I parsed CSV/JSON/JSONL with the Python standard library, used Decimal for time sums, and independently parsed terminal PASS/FAIL records from `evidence/macos-job.log`. I did not run the assembler or any product tests, build, formatter, or native/fixture script. The live CSV was read only for its requested byte count and hash. No delivered artifact was edited.

Each numbered item is one checklist result. Repeated claims are checked together and count once.

## Checklist

1. **CONFIRMED — total, columns, runner counts, and identity join.** The frozen CSV has 3,171 data rows, 40 columns, and 3,171 distinct IDs. Its ID set equals `evidence/inventory.csv` exactly. Runner counts are nextest 2,820; cargo-test 2; node-test 110; python-unittest 39; python-check 200. This settles §§8 and 11 and the repeated totals in §§7 and Numbers and sources.

2. **CONFIRMED — test levels.** Grouping frozen rows by `test_level` gives unit 1,336, integration 886, acceptance 949 (§8).

3. **CONFIRMED — recommendations.** Grouping frozen rows gives keep 3,108, consolidate 56, improve 7, prune_candidate 0, investigate 0 (§§7–10).

4. **CONFIRMED — target kinds and variants.** Frozen rows give integration 1,475; lib 1,269; script-check 200; node 110; bin 78; unittest 39. Variants are default 2,982; local-development 104; production 85 (§8).

5. **CONFIRMED — score distributions.** Independent min/median/max calculations over all frozen rows give criticality_score 25/75/100, usefulness_score 25/80/100, retention_score 31/75/100. The observed ranges in §7 agree with the §8 table.

6. **CONFIRMED — 2,415-second step-covered interval and step sum.** `evidence/job-steps.csv` has 23 data rows. Its durations sum to 2,415 seconds. First step start is `2026-11-08T00:27:42Z`; last completion is `2026-11-08T01:07:57Z`; their difference is 2,415 seconds. The job record's completion is three seconds later. The distinction is explicitly disclosed at methodology lines 384–386.

7. **WRONG — calling that interval the job wall time.** Exact table row at methodology line 376: `| Job wall time | 2415 s = 40 m 15 s | first step start \`00:27:42Z\` to last step completion \`01:07:57Z\` |`. Exact repeated sentence at line 556: “Cumulative test-process seconds (5,931.968 s) are not wall-clock: nextest runs tests concurrently, the Node files run concurrently, and the job wall time is 2,415 s.” Correct values: **job-record wall time is 2,418 s = 40 m 18 s; step-covered wall time is 2,415 s = 40 m 15 s**. Proving source: subtract `jobs[0].startedAt=2026-11-08T00:27:42Z` from `jobs[0].completedAt=2026-11-08T01:08:00Z` in `evidence/run.json`. The same two job timestamps appear in the document's own §1 table and at lines 384–385. The repeated 2,415-second “job wall” label in §§6.3 and Numbers and sources needs the same qualification. This is a labeling error, not an error in the step-duration sum.

8. **CONFIRMED — Test, compilation, execution, and residual.** `job-steps.csv`'s Test row is `00:28:43Z` to `01:04:37Z`, or 2,154 seconds. Raw log line 502 reports `Finished ... in 2m 16s` (136 seconds); line 3396 reports `Summary [2014.918s]`. Decimal subtraction gives `2154 - 136 - 2014.918 = 3.082` seconds (§6).

9. **CONFIRMED — attempts, unique cases, binary count, and cumulative seconds.** Independently stripping ANSI/displayed escapes and parsing PASS/FAIL terminal records between raw log lines 505 and 3396 gives **2,822 records, 2,820 unique (binary_id, test_name) pairs, 56 binaries, and 5,931.968 seconds**. The resulting multiset of binary/name/result/duration tuples equals `evidence/attempts.csv` exactly. SLOW and final FLAKY summaries contribute no extra records. This also verifies §11 check 7's numerical claims.

10. **CONFIRMED — binary cost sums and shares.** Grouping all attempt records by binary reproduces every process-second value in §§6.1 and 10:

    | Binary | Seconds | Share rounded to 2 decimals | Attempt records |
    |---|---:|---:|---:|
    | ovrcr::pi_delegation | 2380.841 | 40.14% | 260 |
    | ovrcr::server_lifecycle | 764.781 | 12.89% | 157 |
    | ovrcr::cursor_cloud | 702.799 | 11.85% | 47 |
    | ovrcr::automatic_titles | 307.713 | 5.19% | 34 |
    | ovrcr::retained_sessions | 219.666 | 3.70% | 22 |
    | ovrcr::claude_quota_probe | 167.450 | 2.82% | 14 |
    | ovrcr-runtime | 154.868 | 2.61% | 496 |
    | ovrcr::terminal_acceptance | 154.485 | 2.60% | 34 |
    | ovrcr::provider_quota | 134.734 | 2.27% | 33 |
    | ovrcr::gui | 99.716 | 1.68% | 13 |

    Basis: `evidence/attempts.csv`, sum of `seconds`, divided by 5931.968 for shares. For every displayed binary other than Pi, the displayed instance count also equals its distinct test-name count.

11. **WRONG — Pi test instances are counted as attempts in §6.1.** Exact table header/row at methodology lines 393–395: `| Binary | Test instances | Process seconds |` and `| \`ovrcr::pi_delegation\` | 260 | 2380.841 |`. Correct value: **259 unique executed test instances, 260 attempt records**. Proving source: filter `evidence/attempts.csv` to `binary_id=ovrcr::pi_delegation`; there are 260 rows but 259 distinct `test_name` values. The frozen CSV likewise has 259 nextest rows for this suite. The parent-recovery retry in §6.2 accounts for the extra attempt. Section 10's value 260 is correct under its different header, “Attempt rows.”

12. **CONFIRMED — both retries.** `attempts.csv` lines 342/345 contain Codex FAIL 4.855, PASS 4.372; sum 9.227 seconds. Lines 717/720 contain Pi FAIL 24.507, PASS 21.743; sum 46.250 seconds. Both frozen rows have first/total/attempts/failed of **4.855/9.227/2/1** and **24.507/46.250/2/1**, respectively. Independent raw-log records occur at lines 848/872 and 1245/1291; final FLAKY reports are lines 3397–3398 (§§6.2, 11).

13. **CONFIRMED — explicit parents and nested TAP durations.** Raw log lines 4428/4481 report parent run durations 5.37/7.25 seconds. Lines 4415/4476 report nested TAP **suite-total** durations 1492.879958/1904.338042 ms. These are the values in §6.3. They are suite totals, not sums of the two per-child `duration_ms` values; nested work lies inside the Rust parent and must not be added to its duration. The frozen CSV has two cargo-test parents and four linked node-test children.

14. **CONFIRMED — top-three, top-fifteen, and slowest-thirty sums.** Sorting attempt-duration sums by binary gives top 3 = **3848.421 s / 64.88%** and top 15 = **5454.380 s / 91.95%**. Sorting measured, parentless frozen rows by `total_attempt_seconds` descending gives the slowest 30 total **1089.436 s / 18.37%** of 5931.968 seconds. This reproduces §10 without relying on `cost-analysis.json`'s summary.

15. **CONFIRMED — criticality of the slowest thirty.** Of that same set, 26 rows have C3, three C4, and one C2; thus 29/30 have C≥3. The exception is `ovrcr::server_lifecycle::desktop_notifications_bridge_expired_permission_check_recovers_without_reauthorization`, 35.784 seconds. All 30 recommendations are keep (§10).

16. **CONFIRMED — overlap counts and the 28-execution group.** `evidence/overlap-groups.json` has **30 groups, 89 member references, 85 distinct member IDs**. `og-001` has 28 members and retains exactly `nextest|Test|ovrcr%3A%3Aautomatic_titles|live%3A%3Acommand_clears_inherited_reporting_authority|default`, the lexicographically smallest member. The frozen CSV preserves 28 rows with the shared `tests/support/live.rs` definition starting at line 568. The remaining 29 groups comprise 26 two-member and three three-member groups. The representative table's `og-008`/`og-012`/`og-019` member counts are 3/2/2 (§9).

17. **CONFIRMED — consolidation totals and surviving protection references.** Frozen rows contain 56 consolidates, eight with C≥3. Every one belongs to an overlap group containing a retained ID whose frozen recommendation is not consolidate/prune_candidate; zero lack such a survivor. This independently reproduces the structural count in validation check 6. It does not independently establish semantic equivalence of every group (§9 and §11).

18. **CONFIRMED — seven rejected proposals.** `evidence/findings-inventory.json["proposal_rejected"]` has seven entries. Their test names match the seven §9 table rows: later-patch admission, the two Agents-Control cases, quota malformed reply, narrow unread card, visible-pane input request, and provider session notification. This verifies the count and listed identities from the named evidence; it is not a fresh source-level adjudication of those seven boundaries.

19. **CONFIRMED — seven improvements and the usefulness floor.** Exactly seven frozen rows have recommendation improve; their names match §10's seven-row table. Exactly one row has usefulness 25: `dashboard::desktop::tests::desktop_confirmed_without_unread_does_not_queue`, C2/S1/U0/F3, consolidate. The next distinct usefulness is 30; the minimum for C≥3 is 35. There are zero prune_candidate rows. These are full-file computations, not deductions from the minimum/median table.

20. **CONFIRMED — review counts.** Frozen rows contain 162 C4 rows and zero low-confidence rows. Required receipt IDs (C4 OR removal recommendation OR low confidence) number **215**, and all 215 occur in canonical `evidence/quality-review.jsonl`. Canonical receipts number 687 records/687 IDs. Segmented `quality-review-*.jsonl` files contain 688 records for the same 687 IDs: the duplicated ID is `nextest|Test|ovrcr%3A%3Atui|unread%3A%3Aan_input_request_never_changes_unread_or_the_review_target|default`, at `quality-review-dashboard.jsonl:56` and `quality-review-shared.jsonl:41`. This settles §8's distinction between receipt records and unique IDs.

    The two segmented records have different checker identities and findings, not identical receipt content: dashboard names `independent-dashboard-check-1`; shared names `independent-shared-check-1`. Both verdicts are agree. The canonical receipt and frozen CSV retain **independent-dashboard-check-1**. No disagreement is demonstrated by these two records, but the duplicate-ID count alone does not establish identical review content.

21. **CONFIRMED — §11's nine-check receipt and its numerical details.** The on-disk `evidence/validation.json["checks"]` lists nine checks, all passed, with the stated counts. Independently: there are 3,171 worker records for 3,171 IDs; all 3,171 score triples recompute; seven rationale fields per row produce 22,197 cells, with zero exact placeholder matches and zero too-short values under the assembler's stated six-field minimum rule; all 3,171 source ranges lie within the existing 250 cited files; and Python csv write/read in memory preserves all 3,171×40 cells. Items 9, 17, 20, and 25 independently settle the remaining numerical assertions. This confirms the receipt's content and recomputed counts, not that its gates implement the entire written policy; see item 27.

22. **CONFIRMED — quoted CSV hash and size, frozen and live.** Methodology line 550 quotes **5,540,928 bytes** and SHA-256 **`d0566038af5be661826c7263c369b09f9052044b47022ad2e13be7cd38b3d5c1`**. Direct byte reads produce exactly those values for both `review/frozen/tests.csv` and the live `tests.csv` at this inspection. The same values appear in `validation.json["csv"]`. The quoted hash therefore still was the live file's hash when checked; concurrent later rewrites are outside this observation.

23. **CONFIRMED — validation-file comparison and determinism statements.** The on-disk `evidence/validation.json` itself is **21,383 bytes**, SHA-256 **`bf115e72667b6ca79427921992077075660baa8ca286d0f71877befd086c90ec`**. Methodology does not quote a size/hash for the validation file itself; its quoted 5,540,928-byte/d0566038… values describe the CSV recorded inside that receipt. The receipt has no ISO timestamp values. The current assembler sorts rows explicitly at `evidence/assemble.py:1504`, consistent with the deterministic-order claim. I did not execute two assemblies.

24. **UNVERIFIABLE — provenance of the old delivery-note hash.** Exact sentence fragment at methodology line 550: “The hash quoted in the delivery note (`ab1c798…`, 5,541,009 bytes) does not match the current file”. The mismatch with today's frozen/live file is confirmed by item 22. The named evidence contains no delivery-note artifact establishing that a delivery note actually quoted those old values. Consequently the historical attribution is unverifiable from the files specified for this assignment; it is not evidence that the present CSV hash is wrong.

25. **CONFIRMED — ascending order, weakest-first score reading, and formulas.** All frozen `(int(retention_score), test_id)` tuples equal their ascending sort, including ID tie-breaks. “Weakest first” is correct specifically for the rubric's retention-score ordering, not an independent verdict about deletion. For every frozen row, `criticality_score=25*C`, `usefulness_score=10*S+10*U+5*F`, and `retention_score=10*C+6*S+6*U+3*F`. Definitions agree with `evidence/rubric.md:5-34` and implementation `evidence/assemble.py:649-652` (§7).

26. **CONFIRMED — cost/retry exclusion from the value formulas.** `rubric.md:34` explicitly says “Runtime, observed retries, and maintenance friction NEVER lower these scores. Score value only.” The assembler formulas at lines 649–652 use only C/S/U/F; no timing, attempt, failure, or cost field enters them. All frozen scores match those formulas. This confirms the documented scoring contract and implementation, not an unobservable claim that human judgments were psychologically unaffected by runtime (§7).

27. **WRONG — the documented policy is not the full policy enforced by the assembler.** Exact policy sentence at methodology line 333: “The recommendation policy is ordered and first match wins:”. Its exact prune rule at lines 341–342 is “**prune_candidate** — high-confidence, C ≤ 2, usefulness ≤ 25, and either no effective behavioral protection or demonstrated complete subsumption by retained tests.” Its consolidate rule at lines 338–340 is “**consolidate** — high-confidence, independently confirmed redundant coverage or repeated execution with existing named retained protection; for C ≥ 3 explicitly document preservation of the high-criticality invariant.” Those rules match `rubric.md:47-52`, but the following code enforces only a different structural subset:

    ```python
    # evidence/assemble.py:898-907
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
    ```

    Lines 908–918 additionally demand that prune overlap IDs be co-members in a confirmed group. Thus the documented alternative “no effective behavioral protection” with no overlap is rejected, even though the written policy allows it.

    ```python
    # evidence/assemble.py:919-924
    elif recommendation == "consolidate":
        consolidate_rows += 1
        if not overlap_list:
            offenders.append(f"{tid}:consolidate without overlap_test_ids")
        elif groups_ok and not member_index.get(tid):
            offenders.append(f"{tid}:consolidate without a confirmed group containing the row")
    ```

    Neither branch requires `confidence == "high"`. Lines 885–888 only require a known confidence enum. Lines 930–977 require valid independent receipts for removal/C4/low-confidence rows; lines 1048–1063 demand nonempty group evidence and surviving retained IDs; lines 1078–1112 check survival chains. None enforce a C≥3-specific explanation of the invariant, first-match recommendation precedence, the improve predicate, or low confidence implying investigate. **Correct description:** the written rubric supplies semantic recommendation policy; the validator enforces enum validity, prune numeric limits, mandatory overlap/group structure for removals, independent receipts, and surviving-protection references. A passed gate is not proof that every written recommendation condition was enforced. The frozen file's 56 consolidates are in fact all high-confidence; this finding concerns the guarantee, not a discovered low-confidence consolidate row.

    The same distinction limits methodology lines 359–360: “Duplicate receipts, receipts for unknown rows, empty reviewer IDs, and self-review fail validation.” `load_model` reads canonical `quality-review.jsonl` when present (`assemble.py:387-398`); segmented files are separately scanned only for diagnostic provenance (`assemble.py:432-445`). The duplicate failure at lines 934–935 applies to loaded canonical receipts, not every segmented receipt. **Correct scope:** duplicates in the canonical input fail; this run's two different segmented checkers for one ID coexist while check 5 passes. The canonical checker retained in the CSV is identified in item 20.

28. **CONFIRMED — author/checker CSV provenance.** Joining every frozen row to `reviews/*.jsonl` on test_id gives **zero reviewer mismatches**: `reviewer` is the shard-author field. Joining to canonical `evidence/quality-review.jsonl` gives **zero second_reviewer mismatches**; receiptless rows have empty second_reviewer. No nonempty second_reviewer equals its author. Current implementation explicitly preserves author provenance at `assemble.py:88-89` and writes checker identity from the receipt at lines 1473–1474/1502. This verifies all 3,171 rows, including resolved receipts.

29. **CONFIRMED — 239 genuinely unmeasured Python rows.** All 39 python-unittest and all 200 python-check frozen rows have `timing_basis=unmeasured` and empty first_attempt_seconds, total_attempt_seconds, attempt_count, and failed_attempt_count. Raw log lines 3631/3669 report only unittest suite aggregates; the method status lines carry no per-method duration. The sequential scripts report check counts rather than per-check elapsed durations. No per-case values can be recovered merely by dividing those aggregates (§§6.3, 11).

30. **CONFIRMED — reported suite aggregates.** Raw log lines 3631/3669 report 33 tests in **10.080 s** and six tests in **2.076 s**. Line 4346 reports Node batch `# duration_ms 4242.999792`, or **4.242999792 s**, which rounds to **4.243 s**. These are the §11 aggregate numbers, not Python case measurements.

31. **CONFIRMED — pinned tracked tree is untouched at inspection.** `git rev-parse HEAD` returned `992a1da5c59a865ebb48051e4d9d8ff7184c8dc5`. `git status --porcelain` returned only `?? .omp/` and `?? research/macos-test-review/`; `git diff --stat` emitted nothing. Thus the tracked checkout currently matches the pinned revision, consistent with no delivered test/assertion/CI-source changes. This does not establish an audit history of every earlier command (§11).

32. **CONFIRMED — single-run and speedup limitations are stated.** Methodology lines 371–372, 429–434, and 555 state that this is one hosted run and establishes no flake rate, variance/percentile, or CI speedup. Lines 420–428 and 532 distinguish cumulative concurrent process durations from possible wall-clock savings and explain nested overlap.

33. **CONFIRMED — remote source observation and score stability.** `src/remote/mod.rs:703-709` exists and contains `if host.starts_with("[::1]") { return true; }`, followed by `host.split(':').next()` and the localhost/127.0.0.1 match. Therefore `[::1].example` satisfies this predicate. `src/remote/mod.rs:144-156` skips configured-login/Tailscale-user-login checks when the predicate is true; a GET request also avoids the later origin restriction at lines 157–163. This supports the report's stated login bypass, without establishing external deployment reachability. Both `findings-inventory.json["product_observation"]` entries describe this same observation. For their two test IDs, original worker and frozen CSV components are unchanged at C4/S3/U3/F3, supporting the claim that this observation did not change those scores.

34. **WRONG — two distinct incidental product observations.** Exact sentence at methodology line 559: “Two incidental product observations were surfaced by the independent review.” Correct value: **two findings entries for one distinct product observation**. Proving source: `evidence/findings-inventory.json["product_observation"]` contains the two adjacent remote-test entries, both describing the same `[::1]` prefix/login-gate issue. The document's own second table row at line 564 explicitly says “there is no independent second product concern”. This is a direct internal contradiction; item 33 confirms the underlying single observation.

35. **WRONG — shared fixture definition extends through line 581.** Exact sentence at methodology lines 154–155: “`tests/support/live.rs` defines a `#[test]` at lines 567–581 (`live::command_clears_inherited_reporting_authority`, source range 568–579).” Correct definition span: **567–579 including the attribute; 568–579 for the function body**, as the parenthetical already says. Proving source: `tests/support/live.rs:567` is `#[test]`, line 568 opens the function, and line 579 closes it; line 581 starts the unrelated `impl Drop for Live`. The same document gives 567–579 in §9 but repeats 567–581 in Numbers and sources. This is a source-range contradiction, not an inventory-row error.

## Internal consistency conclusion

The job-time label conflicts with the document's job-record timestamps; the Pi “Test instances” count conflicts with its own distinction between instances and attempt rows; the two-observation introduction conflicts with its following table; and the shared fixture's 567–581 citation conflicts with its 567–579 citation and actual function boundary. Repeated runner, level, recommendation, score, cumulative-time, retry, overlap, and cost totals otherwise agree at their cited occurrences. The 260 Pi value is valid only where labeled attempt rows. The 2,415-second value is valid only with the step-covered boundary, which the document itself discloses.

The missing historical delivery-note artifact prevents checking that attribution. The current CSV hash is directly confirmed, not blocked by that missing artifact.

**Checklist totals: 29 CONFIRMED; 5 WRONG; 1 UNVERIFIABLE.**

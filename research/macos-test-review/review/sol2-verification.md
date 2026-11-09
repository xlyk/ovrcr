reviewer_model: openai-codex/gpt-6.1-sol

# ALL CONFIRMED

Verified the current `research/macos-test-review/tests.csv`, not the older `d0566038…` snapshot:

- SHA-256: `5b1a9803d927888181554b3ac3b13c1a1bf469ebd2725a9b8ea2fcf0df7a75fd`
- Size: **5,541,169 bytes**; **3,171 rows × 40 columns**.
- The CSV hash remained unchanged after the read-only validator ran.

| # | Claim | Status | Recomputed evidence |
|---|---|---|---|
| 1 | Production-reference syntax and bounds; four retained-session corrections | CONFIRMED | **5,703/5,703** entries match `path:start-end` and resolve within real files; **0** invalid entries. All **4** named retained-session rows contain `crates/ovrcr-runtime/src/retained.rs:157-157`. |
| 2 | Withdrawn quota consolidation | CONFIRMED | **1/1** target row: U=2, `keep`, overlap `[]`; **0** group occurrences. Its signal reason explicitly distinguishes the matrix assertion. |
| 3 | Lost-create-reply reclassification | CONFIRMED | **1/1** target row: `improve`, F=0, U=2; recommendation reason names the missing production tracker assertion. |
| 4 | Local-development `[]` guard signal correction | CONFIRMED | **1/1** target row: S=3; signal reason explicitly says failed output does not establish rejection before IPC. |
| 5 | Narrowed reporting failure claims | CONFIRMED | **2/2** target texts state the exercised boundary: complete write before interruption; at most one assistant per input. Source bodies support both limits. |
| 6 | Totals, receipts, formulas, ordering | CONFIRMED | **3,171** rows; keep **3,108**, consolidate **55**, improve **8**, prune_candidate **0**, investigate **0**. **29** groups / **87** member references / **83** unique IDs. **214/214** required receipts. **9,513/9,513** derived-score cells recompute; all rows are sorted correctly. |
| 7 | Read-only deterministic validation and recorded CSV identity | CONFIRMED | `assemble.py --check` exited **0**, with **9/9** checks passed and status `complete`. `validation.json.csv` records the exact current SHA-256, **5,541,169** bytes and **3,171** rows. |

## Evidence for the applied corrections

### 1. Production references

A standard-library recomputation parsed every row with `csv.DictReader` and every `production_refs` cell with `json.loads`. Each of the 5,703 entries passed a full match of `([^:\n]+):(\d+)-(\d+)`. Each referenced path was a real file, and every range satisfied `1 <= start <= end <= len(file.read_text().splitlines())`. The entries reference 223 distinct files. No bare `path:NNN` citations remain in this column.

These four full IDs contain the corrected `crates/ovrcr-runtime/src/retained.rs:157-157` citation:

```text
nextest|Test|ovrcr%3A%3Aretained_sessions|claude_reopen_retains_exact_conversation_before_another_callback|default
nextest|Test|ovrcr%3A%3Aretained_sessions|claude_task_after_double_dash_starts_once_and_is_never_resent|default
nextest|Test|ovrcr%3A%3Aretained_sessions|codex_reopen_uses_exact_identity_without_prompt_across_two_restarts|default
nextest|Test|ovrcr%3A%3Aretained_sessions|extension_conversations_switch_and_survive_repeated_restart|default
```

The cited line exists: `crates/ovrcr-runtime/src/retained.rs:156-159` defines `create_conversation_schema`, with its SQL statement at line 157.

### 2. Withdrawn consolidation

```text
nextest|Test|ovrcr-runtime|settings%3A%3Atests%3A%3Aquota_in_instance_identity_is_a_finding_and_configures_nothing|default
```

Observed values: `uniqueness=2`, `recommendation=keep`, `overlap_test_ids=[]`. The full ID occurs nowhere in the parsed `evidence/overlap-groups.json`, including member and retained lists.

The current `signal_reason` says:

> The unit body asserts settings.quota stays at the defaults and the exact finding (crates/ovrcr-runtime/src/settings/mod.rs:1536-1544); the neighbour matrix test asserts report rows over a different legacy input (crates/ovrcr-runtime/tests/settings_matrix.rs:778-803), so it does not duplicate this assertion.

Source confirms that distinction. The unit test asserts `report.settings.quota == defaults().quota` and `report.settings.quota.enabled` at `crates/ovrcr-runtime/src/settings/mod.rs:1536-1537`. The matrix counterpart checks `report.rows` and findings, and its legacy input also contains a quota command, at `crates/ovrcr-runtime/tests/settings_matrix.rs:778-803`.

### 3. Lost-create-reply row

```text
node-test|Extension%20tests|tests%2Fpi_workflow_tracker.mjs|lost%20create%20reply%20is%20attributable%20through%20ordinary%20issue%20list%20without%20a%20duplicate%20publication|default
```

Observed values: `recommendation=improve`, `fidelity=0`, `uniqueness=2`. Its recommendation reason includes:

> missing assertion: drive the production tracker path against the real issue-list projection and assert attribution of a lost create reply without a duplicate publication.

`tests/pi_workflow_tracker.mjs:10-31` launches the fixture directly and checks its issue list and ledger. `tests/fixtures/pi/workflow-gh.mjs:15-39` implements that bounded stub; its list branch returns the whole issue array at lines 31-32. The body does not drive the production tracker. This supports the stated missing production-path assertion.

### 4. Guard row

```text
python-check|Relocated%20macOS%20startup%20assets|native%2Fbridge%2Fcheck-guards.py|client-input%5Bb%27%5B%5D%27%5D%3Aln57|local-development
```

Observed value: `signal=3`. The current `signal_reason` says:

> Asserts a failed JSON output and the pinned entitlement digest across 40 executed checks, but the failed-output path does not establish rejection before IPC, so it does not pin the admission boundary itself (native/bridge/check-guards.py:57).

`native/bridge/check-guards.py:38-58` checks return code, stderr, single-line JSON and the `failed` status for this input. It contains no observation of IPC. `native/bridge/main.swift:43-53` has failed-output paths both before IPC and after attempting the remote boundary, supporting the narrowed signal claim.

### 5. Reporting failure descriptions

```text
node-test|Extension%20tests|tests%2Fpi_reporting_extension.mjs|reporting%20fixture%20publishes%20one%20complete%20EOF-terminated%20Unicode%20frame%20before%20interruption|default
```

Current `failure_caught`:

> A frame emitted before its payload was complete, or with a truncated or garbled body; the interruption happens after the complete write, so a mid-write split is not exercised.

The fixture writes the complete newline-terminated body before setting its checkpoint and stopping at `tests/pi_reporting_extension.mjs:62-76`. The test waits for that checkpoint, kills the helper, then checks newline termination and exact payload equality at `tests/pi_reporting_extension.mjs:445-459`. The description explicitly excludes a mid-write interruption claim.

```text
node-test|Extension%20tests|tests%2Fomp_reporting_extension.mjs|error%20and%20aborted%20ends%20settle%20as%20error%20and%20aborted%3B%20no%20assistant%20message%20is%20none|default
```

Current `failure_caught`:

> A wrong outcome or cycle suffix after an error or aborted end, or a fabricated assistant message; each input carries at most one assistant, so first-versus-last selection is untested.

`tests/omp_reporting_extension.mjs:379-397` supplies one error assistant, one aborted assistant, then no messages. It asserts the three run/outcome sequences and prompt exclusion. No input contains competing assistant messages. The revised text states that limit.

## Recomputed totals and validator result

The independent CSV calculation used the rubric formulas from `evidence/rubric.md:33`: `25*C`, `10*S + 10*U + 5*F`, and `10*C + 6*S + 6*U + 3*F`. All 3,171 rows matched all three formulas. Comparing the parsed row sequence with a sort by `(int(retention_score), test_id)` found no ordering differences.

The overlap counts come directly from the parsed `evidence/overlap-groups.json`: 29 objects, the sum of their member-list lengths is 87, and the union of member IDs has size 83.

Required receipt IDs were independently selected from current CSV rows using the gate in `evidence/assemble.py:1007-1019`: C=4, recommendation in `{consolidate, prune_candidate}`, or low confidence. There are 214 distinct required IDs (162 C=4 rows, 55 consolidation rows, with overlap between those sets; no low-confidence rows). All 214 have receipts in `evidence/quality-review.jsonl`, among 687 distinct receipts. Every required receipt has an `agree` or `resolved` verdict, a reviewer matching the CSV's `second_reviewer`, and a reviewer different from the author. No required receipt is missing.

Executed only the read-only validator mode:

```sh
python3 -I research/macos-test-review/evidence/assemble.py --check
```

It exited 0 and reported all nine checks passed: identity join; scores/formulas; rationale quality; source ranges; recommendation gates/reviews; overlap/reference integrity; hosted-evidence gates; CSV round-trip; deterministic order. Its final status was `complete`, with 3,171 rows at revision `992a1da5c59a865ebb48051e4d9d8ff7184c8dc5`.

`evidence/validation.json:510-514` matches the independently calculated current CSV identity exactly. Its nine stored check statuses also all read `passed`. This confirms the requested read-only determinism checks; no writing-mode assembly was run. No product tests, builds, formatters, Python check scripts, or `.mjs` fixtures were executed or imported. This review wrote only this report.

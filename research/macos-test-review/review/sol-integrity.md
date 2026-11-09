reviewer_model: openai-codex/gpt-6.1-sol (thinking level: unknown)

Verdict: FAIL

Four rows violate the required production-reference syntax. Checks 1–5 and 7–9 pass; check 6 fails only on the four known `retained.rs:157` citations.

## Checks

| # | Check | Result | Counts and evidence |
|---|---|---|---|
| 1 | Structure | PASS | 40 columns in the exact `evidence/schema.md:180` order; 3,171 data rows, all with 40 cells; strict UTF-8 decoding succeeds; 0 CR bytes; all 3,171 rows and the header round-trip cell-for-cell through `csv.writer` and `csv.reader`. |
| 2 | Identity | PASS | 3,171 unique IDs equal the 3,171 inventory IDs; 0 duplicates, extras, or missing IDs. All 480 sampled cells match: 30 rows × 10 identity/source fields and six timing fields. |
| 3 | Scoring | PASS | All 12,684 component cells are integers 0–4; all 9,513 derived cells are integers 0–100 and match the three required formulas. 0 mismatches. |
| 4 | Order | PASS | All 3,170 adjacent pairs satisfy ascending numeric `retention_score`, then `test_id`; 0 violations. |
| 5 | Timing | PASS | All 2,820 nextest rows match the ledger's 2,820 groups and 2,822 attempts for first duration, summed duration, attempt count, and failed-attempt count. Exact cumulative attempt time: 5,931.968 seconds. Both retries match. All 956 numeric cells across 239 unmeasured rows are empty. |
| 6 | Cell shapes and references | FAIL | Both array columns parse for all 3,171 rows. Of 5,703 production-reference entries, 5,699 have the exact required shape and valid file/range; four entries on four rows lack the range end. All 2,855 overlap entries resolve without self-reference; all four nonempty parents resolve without self-reference. All 3,171 cost cells satisfy the schema: 2,146 JSON arrays and 1,025 verbatim strings. No other failures. |
| 7 | Placeholders | PASS | 126,840 cells scanned; 0 standalone case-insensitive TBD/TODO/N/A/lorem tokens. All 22,197 behavior, failure, and reason cells have at least 12 characters after trimming; 0 short cells. |
| 8 | Provenance | PASS | All 3,171 CSV reviewers match the sole author record for their ID across 50 shard files containing 3,171 records. All 687 nonempty second reviewers differ from their row's author; 0 self-reviews or author mismatches. |
| 9 | Determinism: assembler `--check` | PASS | Exit code: `0`. Status line: `status: complete; rows: 3171; revision: 992a1da5c59a865ebb48051e4d9d8ff7184c8dc5` |

## Reproduction and scope

Checks 1–8 ran in one `python3 -I -c` process importing only `csv`, `json`, `re`, and `hashlib`. File access used built-in `open`; CSV round-trip used an in-memory sink. Timing comparisons used integer milliseconds, including summing every attempt rather than rounding floating-point sums. A second isolated process confirmed the malformed citations' real file and line.

The audited file is `research/macos-test-review/review/frozen/tests.csv`, 5,540,928 bytes, SHA-256 `d0566038af5be661826c7263c369b09f9052044b47022ad2e13be7cd38b3d5c1`, matching the frozen target. Inventory, attempts, schema, and all 50 `reviews/*.jsonl` files supplied the comparison evidence.

The 30-row sample is reproducible: sort all frozen rows by the hexadecimal SHA-256 of their UTF-8 `test_id` and take the first 30. Compared fields: `revision`, `ci_step`, `runner`, `suite`, `target_kind`, `test_name`, `variant`, `source_file`, `source_start`, `source_end`, `first_attempt_seconds`, `total_attempt_seconds`, `attempt_count`, `failed_attempt_count`, `timing_basis`, and `timing_source`.

The literal command `python3 -I research/macos-test-review/evidence/assemble.py --check` ran without writing mode. That command validates the assembler's live output path; the independent checks above validate the frozen target. Its success does not override the stricter production-reference failures below. No two writing-mode assemblies were run.

`cost_drivers` follows `evidence/schema.md:102-113,183-185`: nonempty strings are stored verbatim and need not be JSON. They are not counted as JSON parse failures.

The model ID comes from runtime project-context metadata. No thinking level was exposed; none is inferred.

## Retry checks

Durations below are seconds; each tuple is first attempt / total attempts / attempt count / failed-attempt count.

- `nextest|Test|ovrcr%3A%3Acodex_initial_prompt|codex_harness_default_and_explicit_prompt_use_the_shared_lifecycle_and_privacy_path|default`: observed `4.855 / 9.227 / 2 / 1`; expected `4.855 / 9.227 / 2 / 1` — PASS.
- `nextest|Test|ovrcr%3A%3Api_delegation|parent_recovery%3A%3Aparent_recovery_transfer_discovers_prepared_and_suspends_accepted_unknown_partial_effects|default`: observed `24.507 / 46.250 / 2 / 1`; expected `24.507 / 46.250 / 2 / 1` — PASS.

## Failures

Each failure is a `production_refs` element. The referenced file exists and has 2,142 lines, so line 157 is valid; only its required `start-end` syntax is wrong.

1. `nextest|Test|ovrcr%3A%3Aretained_sessions|claude_reopen_retains_exact_conversation_before_another_callback|default`
   - Observed: `crates/ovrcr-runtime/src/retained.rs:157`
   - Expected: `crates/ovrcr-runtime/src/retained.rs:157-157`
2. `nextest|Test|ovrcr%3A%3Aretained_sessions|claude_task_after_double_dash_starts_once_and_is_never_resent|default`
   - Observed: `crates/ovrcr-runtime/src/retained.rs:157`
   - Expected: `crates/ovrcr-runtime/src/retained.rs:157-157`
3. `nextest|Test|ovrcr%3A%3Aretained_sessions|codex_reopen_uses_exact_identity_without_prompt_across_two_restarts|default`
   - Observed: `crates/ovrcr-runtime/src/retained.rs:157`
   - Expected: `crates/ovrcr-runtime/src/retained.rs:157-157`
4. `nextest|Test|ovrcr%3A%3Aretained_sessions|extension_conversations_switch_and_survive_repeated_restart|default`
   - Observed: `crates/ovrcr-runtime/src/retained.rs:157`
   - Expected: `crates/ovrcr-runtime/src/retained.rs:157-157`

Distinct offending rows: 4.

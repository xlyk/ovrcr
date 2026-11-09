# Integrity audit — `research/macos-test-review/tests.csv`

- Repo/revision audited: `992a1da5c59a865ebb48051e4d9d8ff7184c8dc5`
- Artifact: `research/macos-test-review/tests.csv` (3,171 data rows, 40 columns)
- Contract: `evidence/schema.md` (structure, serialization), `evidence/rubric.md` (scoring)
- Method: read-only independent checks with `python3 -I` (stdlib `csv`, `json`, `re`, `urllib`, `decimal`, `hashlib`), plus
  `python3 -I research/macos-test-review/evidence/assemble.py --check`. No build/test execution; no file written except this report.

## Verdict

**PASS-WITH-FINDINGS** — 8 of 9 checks pass. One check fails on a reference *format* deviation limited to
4 rows: a `production_refs` item uses a single line (`path:NNN`) instead of the required `path:start-end`.
All referenced files exist and every line number resolves, so no reference is broken; the CSV nonetheless
deviates from the documented cell shape. Distinct offending rows: **4**. No other offending rows.

## Check results

| # | Check | Result | Counts / evidence |
|---|-------|--------|-------------------|
| 1 | Structure (columns, rows, encoding, quoting) | PASS | 40 columns, header exactly matches schema.md order; 3,171 data rows, each 40 cells; valid UTF-8; 0 `\r`, 3,172 LF (one trailing newline); reparse identical; 3,171/3,171 rows round-trip through `csv.writer`; 17,361 cells containing comma/quote/newline round-trip exactly |
| 2 | Identity / inventory join | PASS | 3,171/3,171 unique `test_id`; `tests.csv` ID set == `inventory.csv` ID set (0 extras, 0 missing); 30 evenly sampled rows compared on 14 identity/timing columns = 420 comparisons, 0 mismatches; full-population cross-check of the same 14 columns also 0 mismatched cells |
| 3 | Scores and formulas | PASS | 12,684 component checks (4 per row): all integers, all 0..4; 9,513 derived recomputations (`25C`, `10S+10U+5F`, `10C+6S+6U+3F`): 0 mismatches; all derived scores in 0..100 |
| 4 | Row order `(retention_score, test_id)` ascending | PASS | 3,170 adjacent-pair comparisons, 0 violations |
| 5 | Timing vs `evidence/attempts.csv` | PASS | 2,820/2,820 nextest rows matched by (decoded `binary_id`, `test_name`) to 2,820 unique attempt pairs (all attempts rows consumed, 0 unmatched on either side); 11,280 cell comparisons, 0 mismatches; 239 `unmeasured` rows have all four numeric cells empty (0 non-empty); 2 retried tests verified: codex `4.855 / 9.227 / 2 / 1`, `parent_recovery::...partial_effects` `24.507 / 46.250 / 2 / 1` |
| 6 | Cell shapes and reference integrity | **FAIL** | 5,703 `production_refs` items: 4 items in 4 rows do not match `path:start-end` (single line only). 2,855 `overlap_test_ids` across 1,664 rows: all resolve, 0 self-references, 0 duplicates. 3,171 `cost_drivers` cells parse as JSON array or plain string. 4 `parent_test_id` values all resolve. Detail below. |
| 7 | No placeholders | PASS | 0 cells shorter than 12 characters across `behavior`, `failure_caught` and the four reasons; 0 empty cells; 0 cells equal to a placeholder form. 6 regex token matches across 3 rows were inspected: 5 are the substantive domain word "placeholder" (team-quota placeholder, unescaped-path placeholder) and 1 is a regex false positive (`nA` inside the quoted terminal input `"AA\nA"`). No TBD/TODO/N/A/lorem placeholder text. |
| 8 | Provenance | PASS | 3,171 review records in `reviews/*.jsonl` (50 files), one per CSV row, 0 duplicate `test_id`; `reviewer` equals the shard author for 3,171/3,171 rows; 687 non-empty `second_reviewer`, 0 equal to `reviewer` |
| 9 | Determinism (`assemble.py --check`) | PASS | exit 0; `[PASSED]` for checks 1–9; status line: `status: complete; rows: 3171; revision: 992a1da5c59a865ebb48051e4d9d8ff7184c8dc5` |

## Check 6 failures (only failures)

Observed item vs required shape (`path:start-end`):

| test_id | Observed ref | Expected shape | Resolution |
|---|---|---|---|
| `nextest\|Test\|ovrcr%3A%3Aretained_sessions\|claude_reopen_retains_exact_conversation_before_another_callback\|default` | `crates/ovrcr-runtime/src/retained.rs:157` | `crates/ovrcr-runtime/src/retained.rs:157-157` (or a wider range) | file exists, 2,142 lines; line 157 valid |
| `nextest\|Test\|ovrcr%3A%3Aretained_sessions\|claude_task_after_double_dash_starts_once_and_is_never_resent\|default` | `crates/ovrcr-runtime/src/retained.rs:157` | same | same |
| `nextest\|Test\|ovrcr%3A%3Aretained_sessions\|codex_reopen_uses_exact_identity_without_prompt_across_two_restarts\|default` | `crates/ovrcr-runtime/src/retained.rs:157` | same | same |
| `nextest\|Test\|ovrcr%3A%3Aretained_sessions\|extension_conversations_switch_and_survive_repeated_restart\|default` | `crates/ovrcr-runtime/src/retained.rs:157` | same | same |

Each row's cell is a 4-item JSON array; only the `retained.rs:157` item deviates. The same single-line item is
present in the adjudicated `final_review.production_refs` of the corresponding `quality-review*.jsonl`
receipts (`verdict: "resolved"`), i.e. the shape deviation enters through review input rather than CSV
serialization; serialization itself is consistent (the cell round-trips exactly).

## Scope notes

- Checks were limited to the nine listed checks; no semantic/score-quality judgement was made.
- `tests.csv`, `methodology.md`, `evidence/*` and `reviews/*` were not modified.

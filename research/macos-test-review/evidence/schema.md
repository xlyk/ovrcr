# Assembler input/output schema

`evidence/assemble.py` is the report-local, standard-library-only assembler and
validator for `research/macos-test-review/tests.csv`. It joins frozen evidence,
recomputes every score, writes the CSV, and records the validation result in
`evidence/validation.json`. It never builds, tests, or modifies product code,
tests, workflow, or configuration.

Run everything from the repository root with an isolated interpreter:

```sh
python3 -I research/macos-test-review/evidence/assemble.py            # assemble + validate
python3 -I research/macos-test-review/evidence/assemble.py --check    # validate only, writes nothing
python3 -I research/macos-test-review/evidence/assemble.py --audit    # diagnostic offender lists → evidence/audit-details.json
python3 -I research/macos-test-review/evidence/assemble.py --selftest # disposable fixture proofs
```

| Mode | Writes | Exit 0 | Exit 1 | Exit 2 |
|---|---|---|---|---|
| default | `tests.csv`, `evidence/validation.json` | status `complete` or `complete-with-evidence-gaps` | status `failed` | missing/malformed required input; **nothing written** |
| `--check` | nothing | structural validation passed (gaps allowed) | any check `failed` | missing/malformed required input |
| `--audit` | only `evidence/audit-details.json` | audit written | — | missing inventory/reviews |
| `--selftest` | only `evidence/selftest/**` | every fixture behaved as expected | a fixture proof failed | fixture load failure |

`--audit` is a diagnostic, not a gate: it tolerates an absent
`overlap-groups.json` (treated as `[]` and noted), reads
`quality-review.jsonl` when present (otherwise merges segmented
`quality-review-*.jsonl` in memory for coverage), and independently scans the
segmented receipts only to report duplicate provenance. It does not feed
segment-only copies into the receipt set used for missing-receipt detection, and
it writes no CSV or validation report.

A failed default run writes no `tests.csv`; an existing one is removed so a stale
report is never left in place. `validation.json` is still written with the failed
checks and offending IDs.

## 1. Canonical inventory — `evidence/inventory.csv`

UTF-8 CSV with exactly this header and order (RFC 4180, read with the `csv` module):

```text
test_id,parent_test_id,revision,ci_step,runner,suite,target_kind,test_name,variant,source_file,source_start,source_end,first_attempt_seconds,total_attempt_seconds,attempt_count,failed_attempt_count,timing_basis,timing_source
```

- `test_id` — unique, non-empty, stable join key.
- `parent_test_id` — empty, or an ID present in this same CSV (never itself).
- `revision` — the pinned revision; one distinct value across the whole file.
- `ci_step`, `suite`, `test_name`, `variant`, `source_file`, `timing_basis` — non-empty strings.
- `runner` — one of `nextest`, `cargo-test`, `node-test`, `python-unittest`, `python-check`.
- `target_kind` — one of `lib`, `bin`, `integration`, `node`, `unittest`, `script-check`, `doctest`.
- `source_start`/`source_end` — integers, `1 <= start <= end <= file line count`.
- `first_attempt_seconds`/`total_attempt_seconds` — measured seconds as text; empty when unmeasured (never `0`).
- `attempt_count`/`failed_attempt_count` — observed counts.
- `timing_basis`/`timing_source` — provenance; unknown timing stays empty, not zero.

Identity columns (`parent_test_id`, `revision`, `ci_step`, `runner`, `suite`,
`target_kind`, `test_name`, `variant`, `source_file`, `source_start`,
`source_end`) must never disagree between inventory and a review record.

**Compatibility path.** When `evidence/inventory.csv` is absent, the assembler
imports `evidence/inventory_rust.csv` (the inventory workstream's intermediate)
with header:

```text
test_id,input_test_name,invocation,runner,suite,target_kind,test_level,test_name,variant,source_file,source_start,source_end,first_attempt_seconds,total_attempt_seconds,attempt_count,failed_attempt_count,timing_basis,timing_source
```

Mapping: `ci_step <- invocation`, `revision <-` manifest revision or read-only
`git rev-parse HEAD`, `parent_test_id <- ""`; `input_test_name` and the
intermediate `test_level` are ignored (the worker review supplies `test_level`).
`validation.json` records `inventory_source` and a note whenever this path is
used. The canonical header is the contract; the fallback exists so assembly is
not blocked while the merged inventory lands. The fallback refuses to run when
`evidence/inventory_node.csv` or `evidence/inventory_python.csv` also exist —
importing the Rust fragment alone would silently drop those rows, so the merged
`evidence/inventory.csv` must be built instead.

## 2. Worker reviews — `reviews/<shard-id>.jsonl`

One JSON object per line. Exactly one record per inventory `test_id`; duplicates,
extra records, and missing records fail. Every field is required:

```json
{
  "test_id": "…",
  "test_level": "unit|integration|acceptance",
  "production_refs": ["path:start-end", "…"],
  "behavior": "concrete behavior asserted",
  "failure_caught": "concrete plausible defect detected",
  "criticality": 0, "signal": 0, "uniqueness": 0, "fidelity": 0,
  "criticality_reason": "…", "signal_reason": "…",
  "uniqueness_reason": "…", "fidelity_reason": "…",
  "confidence": "high|medium|low",
  "recommendation": "investigate|consolidate|prune_candidate|improve|keep",
  "recommendation_reason": "…",
  "overlap_test_ids": ["…"],
  "cost_drivers": "evidenced observations",
  "reviewer": "authoring agent id"
}
```

- Scores are JSON integers `0..4` (strings and booleans are rejected).
- `production_refs` and `overlap_test_ids` are native JSON arrays of strings and
  are re-serialized into CSV cells with `json.dumps(list, ensure_ascii=False)`.
- `cost_drivers` is optional and describes observed cost mechanisms. Accepted
  shapes: a non-empty string; an array of 1..N non-empty strings; or an empty
  form (`[]`, `""`, `null`, or an omitted field) meaning "no observed cost
  driver". Empty forms serialize to the CSV cell `[]`. A non-empty string is
  written verbatim; a non-empty array is serialized as a JSON array with
  `json.dumps(value, ensure_ascii=False)`, matching `production_refs` and
  `overlap_test_ids`. An array containing an empty or non-string element, or any
  other type, fails with the offending `test_id`. The field never contributes to
  scoring, so an empty cell is not a quality failure.
- Derived scores must not be trusted from input: `criticality_score`,
  `usefulness_score`, and `retention_score` are always recomputed, and any
  supplied value that disagrees fails validation.

## 3. Overlap groups — `evidence/overlap-groups.json` (required)

JSON array; write `[]` when there are no confirmed groups:

```json
[{
  "group_id": "unique non-empty string",
  "member_test_ids": ["…"],
  "retained_test_ids": ["…"],
  "preservation_evidence": "assertion/source references and boundary comparison"
}]
```

Structural rules: unique `group_id`; members non-empty, unique, and resolvable to
inventory rows; retained IDs non-empty and a subset of members;
`preservation_evidence` non-empty (string, array, or object). A group must retain
at least one member whose recommendation is not `prune_candidate` or
`consolidate`. Every `overlap_test_ids` entry in every row must resolve and must
not name its own row.

**No removal may erase protection.** A row recommended `prune_candidate` or
`consolidate` must reach a retained protector that is not itself a removal
candidate, following group-retained edges as needed. A cycle such as “A is
retained only by B and B only by A” fails with both IDs and the cycle path.

## 4. Independent-review receipts — `evidence/quality-review.jsonl`

```json
{"test_id": "…", "second_reviewer": "…", "verdict": "agree|resolved",
 "findings": "non-empty text or non-empty array/object",
 "final_review": null}
```

- `verdict: "agree"` requires `final_review: null`.
- `verdict: "resolved"` requires a `final_review` object; a resolved receipt with
  `null` final_review is a failure.
- `final_review` is the adjudicated semantic record. Any semantic field it
  contains overrides the worker record **except `reviewer`**, which is
  provenance and always stays the shard author from `reviews/*.jsonl`; identity
  fields and `test_id` in it must match inventory. Unlisted fields keep the
  worker value.
- Receipts are required for every row with `criticality == 4`, every
  `prune_candidate`/`consolidate` row, and every `low`-confidence row.
- Duplicate receipts, receipts for unknown rows, an empty `second_reviewer`, or a
  `second_reviewer` equal to the worker `reviewer` fail.
- The CSV `second_reviewer` column comes from the receipt and is empty when there
  is none.

`evidence/calibration.jsonl` is read only for reporting (receipt count); it never
contributes scores.

## 5. Derived scores and CSV serialization

```text
criticality_score = 25*C
usefulness_score  = 10*S + 10*U + 5*F
retention_score   = 10*C + 6*S + 6*U + 3*F
```

Output header (exact order, UTF-8, RFC 4180 via `csv`, LF line endings):

```text
test_id,parent_test_id,revision,ci_step,runner,suite,target_kind,test_level,test_name,variant,source_file,source_start,source_end,production_refs,behavior,failure_caught,criticality,signal,uniqueness,fidelity,criticality_score,usefulness_score,retention_score,criticality_reason,signal_reason,uniqueness_reason,fidelity_reason,confidence,recommendation,recommendation_reason,overlap_test_ids,cost_drivers,first_attempt_seconds,total_attempt_seconds,attempt_count,failed_attempt_count,timing_basis,timing_source,reviewer,second_reviewer
```

Rows are sorted by `(retention_score, test_id)` ascending. Arrays are JSON in
their cell (`production_refs`, `overlap_test_ids`, and array-form
`cost_drivers`); a string-form `cost_drivers` is written verbatim; unknown
measurements remain empty. The CSV is written atomically,
read back with the `csv` module, and every cell must equal the in-memory value
(commas, double quotes, newlines, and non-ASCII included).

## 6. Hosted evidence (optional, mandatory when present)

- `evidence/attempts.csv` — header
  `invocation,binary_id,test_name,attempt,result,seconds,raw_line`. When present,
  the assembler recomputes and asserts: 2822 attempt records, 2820 unique
  `(binary_id,test_name)` pairs, 56 binaries, cumulative seconds `5931.968`
  (±0.001), and both retry rows — Codex initial prompt `4.855 / 9.227 / 2 / 1`
  (first / total / attempts / failed) and pi
  `parent_recovery::parent_recovery_transfer_discovers_prepared_and_suspends_accepted_unknown_partial_effects`
  `24.507 / 46.250 / 2 / 1`.
- `evidence/manifest.json` — any recorded revision is compared with the inventory
  revision; any `sha256` records are verified against the files that exist; gate
  values under recognized key names (`attempt_records`/`attempts`,
  `unique_tests`/`unique_rust_ids`, `binaries`, `cumulative_seconds`/`sum_seconds`)
  and structured retry entries are compared with recomputation. Unrecognized
  shapes are not guessed at.
- `evidence/macos-job.log` — presence cross-check for `2820 tests run` and both
  retried test names after stripping ANSI and displayed `^[[…m` escapes.

If `attempts.csv` is absent, check 7 is `unverified` and the overall status is
`complete-with-evidence-gaps` (exit 0 means structural validation passed, not
that hosted gates passed). If it is present but inconsistent, the run fails.

## 7. Validation checks recorded in `evidence/validation.json`

| # | Check | Fails when |
|---|---|---|
| 1 | `row-count-and-identity-join` | duplicate/missing/extra reviews, duplicate inventory IDs, identity conflicts, empty required fields, bad enums, parent does not resolve or self-parent, mixed revisions, or `reviewer == second_reviewer` (provenance must name the shard author, never the checker); reports per-runner/per-step/per-variant counts and asserts their sum |
| 2 | `scores-and-formulas` | score missing/not an integer/outside 0..4, or a supplied derived score differs from recomputation |
| 3 | `rationale-quality` | a rationale cell (behavior, failure_caught, the four score reasons, recommendation_reason) is empty or matches `^(tbd\|todo\|n/?a\|none\|unknown\|placeholder\|-\|)$` (case-insensitive), or one of behavior/failure_caught/the four reasons is shorter than 12 characters; `cost_drivers` is optional, accepts empty forms (`[]`/`""`/null/omitted), and is an observation field, so `none` is allowed there |
| 4 | `source-ranges` | `source_file` missing, outside the repository, or `source_start`/`source_end` non-integer, inverted, or beyond the file's line count |
| 5 | `recommendation-gates-and-reviews` | prune without `C<=2` or `usefulness<=25` or non-empty overlap co-membered in a confirmed group; consolidate without overlap and a group containing the row; invalid confidence/recommendation; required receipt missing or malformed; self-review |
| 6 | `overlap-groups-and-reference-integrity` | invalid group structure, unresolvable/self overlap references, no retained non-candidate, removal candidate without surviving protection, or a circular sole-survivor chain |
| 7 | `hosted-evidence-gates` | any hosted gate above mismatches; `unverified` only when `attempts.csv` is absent |
| 8 | `csv-round-trip` | rendered/written CSV does not parse back cell-for-cell, contains CR, or is not UTF-8 |
| 9 | `deterministic-order` | written row order differs from `(retention_score, test_id)` ascending |

Each entry is `{check, status: passed|failed|unverified, detail, offending_ids}`.
`validation.json` also records `revision`, input file `sha256`+size, command
line, mode, `inventory_source`, row counts (total and by runner / CI step /
variant / recommendation / test level / target kind), min/median/max for the three
derived scores, formula weights, receipt
counts, the written CSV's hash/size, and notes. It contains no timestamps, so
identical evidence produces byte-identical output.

### 7.1 Diagnostic audit — `evidence/audit-details.json`

`--audit` writes dispatch-ready offender lists (no CSV, no validation report):

```json
{
  "schema": "macos-test-review/audit-details@1",
  "revision": "…",
  "inventory_source": "evidence/inventory.csv",
  "receipt_input_files": ["evidence/quality-review.jsonl"],
  "segment_files_scanned": ["evidence/quality-review-*.jsonl", "…"],
  "check_statuses": [{"check": "…", "status": "…", "detail": "…"}],
  "counts": {"missing_required_receipt": 0, "self_review": 0, "duplicate_receipt": 0,
             "consolidate_rows": 0, "consolidate_without_group": 0,
             "self_overlap": 0, "other_offenders": 0},
  "missing_required_receipt": [{"test_id", "shard_id", "review_file", "review_line",
                                "author_reviewer", "reviewer", "criticality",
                                "recommendation", "confidence"}],
  "self_review": [{"test_id", "reviewer", "second_reviewer", "receipt_files"}],
  "duplicate_receipt": [{"test_id", "receipt_count", "receipt_files", "receipt_locations",
                         "merged_kept_second_reviewer", "merged_kept_verdict"}],
  "consolidate_rows": [{"test_id", "shard_id", "author_reviewer", "overlap_test_ids",
                        "reviewer", "criticality", "confirmed_group_containing_row",
                        "surviving_protection"}],
  "self_overlap": [{"test_id", "overlap_test_ids"}],
  "other_offenders": [{"check", "offender"}]
}
```

Self-review compares `second_reviewer` with the **shard author** (`reviewer` from
`reviews/*.jsonl`), not the possibly adjudicated `reviewer` inside a
`final_review`. Duplicate receipts are detected by scanning the segmented
`quality-review-*.jsonl` files, naming every file and line plus each copy's
`second_reviewer`/`verdict`; `receipt_locations` and the `merged_kept_*` fields
expose whether the copies actually agree, so a disagreement is never mistaken
for a harmless byte-identical write. Segment-only copies are never added to the
missing-receipt calculation. The `consolidate_rows` array contains **every**
consolidate row annotated with `confirmed_group_containing_row` and
`surviving_protection`; the counts object separately reports `consolidate_rows`
(all) and `consolidate_without_group` (those lacking a confirmed group).
Offenders already covered by the first five lists are omitted from
`other_offenders`, so every entry appears exactly once.

## 8. Selftest — `evidence/selftest/`

`--selftest` deletes and regenerates disposable fixtures under
`evidence/selftest/<case>/` and proves, naming the relevant IDs:

| Fixture | Proof |
|---|---|
| `duplicate-test-id` | check 1 fails naming the duplicated ID |
| `missing-review` | check 1 fails naming the uncovered ID |
| `invalid-score` | check 2 fails naming the row with score 5 |
| `invalid-cost-drivers` | check 1 fails naming a row whose array contains an empty string and a row whose `cost_drivers` is a number |
| `formula-mismatch` | check 2 fails naming the row whose adjudicated `retention_score` contradicts the formula |
| `dangling-overlap` | check 6 fails naming the row with the unresolvable overlap ID |
| `missing-second-review` | check 5 fails naming the prune, C=4, and low-confidence rows without receipts |
| `circular-sole-survivor` | check 6 fails naming both IDs and the cycle path |
| `pass-tricky-cells` | structural checks pass; a cell with a comma, double quote, newline, and `emoji ✅ / café` round-trips exactly; `cost_drivers` round-trips as a plain string, a JSON array, and all empty forms (`[]`/`""`/null/omitted → `[]`) |
| `hosted-gates-pass` | checks 1-9 pass on a synthetic ledger with the frozen gate values; `validation.json` has all required keys; status `complete` |
| `hosted-gates-mismatch` | check 7 fails on a manifest gate claim that disagrees with recomputation |
| `check-mode-read-only` | `--check` exits 0 on a valid fixture, exits 1 on the failing fixture, and writes neither file |

The fixture rows cite a real repository file (`Cargo.toml:1-3`) so source
validation is genuine. Fixtures live only under `evidence/selftest/`, which the
normal run never reads: normal mode reads exactly `evidence/inventory.csv`
(or the documented compatibility file) and `reviews/*.jsonl`. The selftest also
snapshots the tree outside `evidence/selftest/` and fails if any fixture write
escaped.

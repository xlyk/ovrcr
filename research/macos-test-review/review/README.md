# Independent review of this deliverable

Three review rounds ran against `research/macos-test-review/tests.csv`. Each was an
independent team that did not author the work. Findings were triaged; only those
defensible from source were applied.

## Reviewers and models

| Round | Agents | Model |
|---|---|---|
| 1 | `SolIntegrity`, `SolClaims`, `SolJudgment` | `openai-codex/gpt-6.1-sol` at spawn level `:high` (reports record thinking level as `unknown`) |
| 1 (partial, discarded) | `audit-integrity`, `audit-claims`, `audit-judgment` | `deepseek/deepseek-v4-flash:high` and `:low` — useful, but not the requested model; superseded by round 1 |
| 2 | `SolVerify2`, `SolClaims3`, `SolJudgment3` | `openai-codex/gpt-6.1-sol` at spawn level `:medium` |

The medium/high distinction is the spawn parameter, not a claim about the model's own
reasoning effort: neither round's reports could observe their thinking level, and both
record `reviewer_model` from runtime project-context metadata only.

## Round 1 — findings and resolutions

Reports: `sol-integrity.md`, `sol-claims.md`, `sol-judgment.md`.

**Integrity** — 8 of 9 checks passed. Sole failure: four rows carried
`crates/ovrcr-runtime/src/retained.rs:157` instead of `path:start-end`.

Resolved: the four entries were normalised at source in the quality-review receipts;
`evidence/assemble.py` now validates the `production_refs` shape, path existence and
range bounds (it previously did not check them at all), with negative cases proven
in-process for single-line, out-of-bounds and missing-file forms.

**Claims** — 29 confirmed, 5 wrong, 1 unverifiable. All corrected in `methodology.md`:

| Was | Now |
|---|---|
| "Job wall time 2415 s" | step-covered interval 2415 s; job-record wall time 2418 s |
| `ovrcr::pi_delegation` 260 test instances | 259 unique instances over 260 attempt rows |
| "Two incidental product observations" | one observation, two inventory entries |
| `tests/support/live.rs:567–581` | `567–579` (body `568–579`) |
| delivery-note hash claim (`ab1c798…`) | dropped as unverifiable |
| policy implied fully enforced | validator enforces structural gates only |

**Judgment** — 19 agree, 5 disagree: 1 blocker, 2 concerns, 2 nits.

| Row | Change |
|---|---|
| `settings::tests::quota_in_instance_identity_is_a_finding_and_configures_nothing` | consolidation **withdrawn** (retained test compares `report.rows`, not effective `report.settings.quota`); now `keep`, U=2, group `og-012` deleted |
| `pi_workflow_tracker::lost create reply is attributable…` | `keep` → `improve` (F=0 and U=2 meet the improve gate) |
| `check-guards.py::client-input[b'[]']:ln57` (local-development) | S 4 → 3 (failed output does not establish rejection before IPC) |
| `pi_reporting_extension` frame row, `omp_reporting_extension` outcome row | `failure_caught` narrowed to what the body actually exercises |

Post-fix totals: keep 3,109 / consolidate 54 / improve 8 / prune 0 / investigate 0;
28 overlap groups (85 member references, 81 unique ids); 213/213 required receipts.

Round 2 added one more withdrawal: `task_tui::destructive_actions_require_confirmation_and_escape_cancels`
was marked `consolidate`/U=0 against `task_confirmations_honor_default_no_and_case_insensitive_answers`,
but the survivor builds a **fresh view for every answer** (`tests/task_tui.rs:471-536`) and therefore never
observes a cancellation followed by a successful confirmation on one instance — the sequence the candidate
exercises at `tests/task_tui.rs:231-242`. Uniqueness raised to 2, recommendation `keep`, group `og-002`
deleted.

## Round 2 — scope

Reports: `sol2-verification.md`, `sol2-claims.md`, `sol2-judgment.md`.

Round 2 examines the **post-fix** artifact rather than repeating round 1:
verification re-checks each applied correction in the current CSV; the claims audit
re-verifies the rewritten methodology; the judgment pass samples 24 rows that round 1
did not judge, weighted toward the eight `improve` rows and the C>=3 rows nearest the
prune gate.

## Known gaps in this deliverable

- Six of the eight `improve` rows name the missing assertion but cite no `file:line`.
  `recommendation_reason` is not a scored field and the validator does not require a
  citation there, so they were left as written rather than given invented paths.
- Per-case timing is unavailable for the 39 python-unittest methods and 200 script-check
  rows; their timing cells are empty with `timing_basis=unmeasured`, and only suite
  aggregates exist.
- Every timing statement derives from one hosted run. It establishes neither a flake rate
  nor a CI speedup, and concurrent process durations do not sum to wall-clock.
- One incidental product observation is recorded twice in `findings-inventory.json`
  (the `[::1]` prefix fail-open); it is one issue with two inventory entries, not two.

## Artifact identity

| Stage | sha256 | bytes |
|---|---|---|
| Round-1 snapshot (`review/frozen/tests.csv`) | `d0566038af5be661826c7263c369b09f9052044b47022ad2e13be7cd38b3d5c1` | 5,540,928 |
| Post-round-1 rebuild | `5b1a9803d927888181554b3ac3b13c1a1bf469ebd2725a9b8ea2fcf0df7a75fd` | 5,541,169 |
| Post-round-2 first rebuild | `763e111b30e562356fead4235164e927cf8eac9f8c5c2bc51b858bef115a3cfe` | 5,541,175 |
| Post-round-2 final build (current) | `dcccdb0ec9d19a3569a05285f33f7620e71308f3765edf2d0086996ca767fb5a` | 5,541,668 |

`evidence/validation.json` records the hash of the file actually written, and the
assembler is deterministic: two consecutive assemblies produce byte-identical output.

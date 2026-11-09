# Methodology (result-independent draft)

This draft documents only the parts of the study that do not depend on review results:
the frozen CI evidence, the scope and count reconciliation, the scoring rubric, the
output columns, the recommendation gates, and the measured job cost. The findings,
score distributions, recommendations, and cost-benefit conclusions are deliberately
omitted; a later pass appends them. Every number below is taken from the evidence
files named in the final "Numbers and sources" section, not from memory.

## 1. Frozen evidence

The study is pinned to one revision and one completed GitHub Actions job.

| Item | Value |
|---|---|
| Revision | `992a1da5c59a865ebb48051e4d9d8ff7184c8dc5` |
| Branch | `chore/unit-test-review` |
| Workflow run | `42429838269` |
| Job | `128576086691`, name `macOS checks` |
| Job URL | `https://github.com/xlyk/ovrcr/actions/runs/42429838269/job/128576086691` |
| Conclusion | `success` |
| Head SHA | `992a1da5c59a865ebb48051e4d9d8ff7184c8dc5` |
| Job started / completed (UTC) | `2026-11-08T00:27:42Z` / `2026-11-08T01:08:00Z` |

The log and run JSON were retrieved with these exact commands:

```sh
gh run view 42429838269 --job 128576086691 --log \
  > research/macos-test-review/evidence/macos-job.log
gh run view 42429838269 --json conclusion,headSha,jobs \
  > research/macos-test-review/evidence/run.json
```

Retrieval times were `2026-11-08T06:45:51Z` for `evidence/macos-job.log` and
`2026-11-08T06:45:47Z` for `evidence/run.json`. These are filesystem mtimes converted
to UTC; both files were already present and complete when the study started and were
not refetched. The log ends with job cleanup and lists all steps; `run.json` lists the
23 steps and the job conclusion.

Every file in the frozen set has a recorded SHA-256 in `evidence/manifest.json`:

| File | Bytes | SHA-256 |
|---|---:|---|
| `evidence/macos-job.log` | 867305 | `159592ad3a4562507b013c5ce3e0e4f057e83622b678fb82f61e175be1bef9e2` |
| `evidence/run.json` | 7155 | `a32b77493e71cd63690e763950828cc9e69c64b8f1c9ab95d0010153f0c4f55e` |
| `evidence/attempts.csv` | 990680 | `dd7c9991fb3ca9d69d5554771b4a22aec0112fe61023ec29c38ba15a3585d0c4` |
| `evidence/inventory_rust.csv` | 1367470 | `23ad9a3cff2af1232b104dd9afe0864004e29f3d5e5c17c9bc902f0e225cb6d3` |
| `evidence/inventory_rust_test_level.csv` | 371095 | `ed1b558f58f2b9615fe3ab571b6ab788c405f94c3bdff3f874076574e921f4b0` |
| `evidence/inventory_node.csv` | 60604 | `38bbc73d7126b5567bb55154a0bf016a9b411e3dd4a524fdd92daf609110c289` |
| `evidence/inventory_python.csv` | 164897 | `be0fc2329e016ca25080095604a3f33d414b102ec0cb19362d30ce6c6ec65b6a` |
| `evidence/inventory.csv` | 1470463 | `e813fbe190e4d562572caefceadfee696e275c65cb2a4d825b88a09fb9921205` |
| `evidence/job-steps.csv` | 4759 | `a9ff1f92d5b53967eaaf862b38777be32722661dc606a87ae535c4caf3111f9d` |
| `evidence/exclusions.csv` | 9395 | `7aef319ba0200b234d12078b26aeb26b8ea94194c224768edcc57c90a7a151f5` |
| `evidence/parse_log.py` | 5865 | `adfe3b2c4a58443a2ff8b2373f529debefec6030443ad109633d73c3da21ee89` |
| `evidence/build_inventory.py` | 50230 | `d55f1250dec919866b47b5a68eaa8162504d790e4234b1dc8d6017a783441bf0` |

Only the listed files have recorded hashes; `evidence/rubric.md`, `evidence/schema.md`,
`evidence/node-notes.md`, `evidence/python-checks.md`, and the review inputs are
supporting documents, not captures.

The tracked tree at the pinned revision is unmodified: `git rev-parse HEAD` returns the
pinned SHA and `git status --porcelain` reports only the untracked `research/` workspace
and `.omp/`. No test was deleted, no assertion was weakened, and no CI selection was
changed by this study. The study only read the frozen log, run JSON, and repository
sources, and wrote review records and this report material under
`research/macos-test-review/`.

## 2. Scope and count reconciliation

### 2.1 The job's real command set

The scope is the `macOS checks` job (`macos_checks`), not the whole workflow. Its steps
and commands are:

| Step | Command |
|---|---|
| Format | `cargo fmt --all -- --check` |
| Lint | `cargo clippy --workspace --all-targets --all-features -- -D warnings` |
| Test | `cargo nextest run --workspace --all-targets --all-features --profile ci -E 'not test(partial_resize_connection_delivers_error_before_owner_close)'` |
| Doc tests | `cargo test --workspace --doc` |
| Relocated macOS startup assets | `cargo build -p ovrcr --bin ovrcr`; `sh scripts/package-startup.sh …`; `python3 -I native/bridge/check-local-startup.py`, `check-installer.py`, `check-startup-package.py`, `check-packaging.py`, `check-guards.py`, with the last three repeated under `--local-development` |
| Relocated pinned Cursor dependency | `python3 -I scripts/install-cursor-bridge.py`; `python3 -I tests/cursor_bridge_install.py` |
| Extension tests | one `node --test` over nine `.mjs` files: `pi_package_bootstrap`, `pi_reporting_extension`, `omp_reporting_extension`, `pi_delegation_extension`, `pi_interrupt_extension`, `pi_question_extension`, `pi_continuation_records`, `pi_observation_extension`, `pi_workflow_tracker` |
| Provision pinned Pi loader test host | `npm install --prefix "$RUNNER_TEMP/ovrcr-pi-package-host" … @earendil-works/pi-coding-agent@0.87.1` |
| Installed Pi package through production materialization | `cargo test -p ovrcr --test pi_package --features acceptance-diagnostics production_materializer_loads_local_package_with_one_injected_owner -- --ignored --exact --nocapture --test-threads=1` |
| Installed Pi effective child tools through production assignment argv | `cargo test -p ovrcr --test pi_delegation --features acceptance-diagnostics coding::production_assignment_argv_preserves_installed_sdk_child_publication_tools -- --ignored --exact --nocapture --test-threads=1` |

Format, Lint, Test, and the Python/Node package checks are read-only or check-only; no
step mutates product sources. The `--all-features` flag is load-bearing: it compiles and
selects the `gui`, `required-features = ["gui"]`, and `acceptance-diagnostics` targets,
so feature-gated tests are in scope rather than excluded.

### 2.2 Inventory reconciliation

`evidence/inventory.csv` has 3171 rows, one per execution ID:

| Runner | Rows | Composition |
|---|---:|---|
| `nextest` | 2820 | one row per nextest test instance in 56 binaries; 33 skipped |
| `cargo-test` | 2 | the two ignored Pi parents, each run explicitly by its own step |
| `node-test` | 110 | 106 cases from the nine-file `node --test` batch + 4 nested cases |
| `python-unittest` | 39 | 33 methods in `check-installer.py`, 6 in `tests/cursor_bridge_install.py` |
| `python-check` | 200 | logical checks in the five `native/bridge/check-*.py` runs |
| **Total** | **3171** | |

The nextest count reconciles with the log: `Starting 2820 tests across 56 binaries
(33 tests skipped)`. The 2853 compiled tests are `2820 run + 32 source-ignored + 1
filtered`; the 33 skipped are those 32 `#[ignore]` tests plus the single `-E`-filtered
test. Four additional tests are `#[cfg(not(target_os = "macos"))]` and were never
compiled on this runner. The two Pi parents are among the 32 source-ignored tests but
were executed separately by the two `cargo test … --ignored --exact` steps; they are
therefore present both in the exclusion inventory (as ignored from the Test step) and in
the scored inventory (as `cargo-test` rows).

The four nested Node rows are children of the two `cargo-test` parents: each Rust parent
spawns `node --test` on a dedicated `.mjs` file, and the nested TAP stream appears in the
parent's `--nocapture` output. Each parent produced two nested cases, so the rows carry
the parent's full `test_id` in `parent_test_id`; `node-notes.md` records the parent IDs,
TAP line ranges, durations, and source slots. The 106 batch cases are a different
population: they come from the nine-file invocation and have empty `parent_test_id`
(except where a case is nested under one of the two explicit steps, which the 4 rows
above cover).

The Doctests step runs five per-crate sections (`ovrcr`, `ovrcr_protocol`,
`ovrcr_runtime`, `ovrcr_terminal`, `ovrcr_tui`). Each prints `running 0 tests`, so the
step produces no test rows and no doctest rows appear in the inventory.

### 2.3 Deliberately excluded material

`evidence/exclusions.csv` is the complete exclusion ledger (41 data rows). Its categories:

- **nextest filter** (1 row): `server::tests::partial_resize_connection_delivers_error_before_owner_close`
  is removed by the `-E` expression. This is why 2820 ran while 2853 were compiled.
- **ignored-test** (32 rows): source `#[ignore]` tests. They are counted in the log's 33
  skipped; the two Pi parents among them were later run by the dedicated steps.
- **cfg-platform** (4 rows): `#[cfg(not(target_os = "macos"))]` tests, three in
  `ovrcr-tui::dashboard::desktop` and one in `ovrcr::server_lifecycle`. They are not
  compiled on this runner and therefore have no execution ID.
- **feature-gate** (1 row): a record that `--all-features` did *not* exclude a target;
  it is a scope note, not an exclusion.
- **node-suite** (1 row): `tests/pi_publish_contract_installed.mjs` is not invoked by
  this job. It is absent from the nine-file batch and from both nested steps.
- **doctest** (1 row): the `cargo test --workspace --doc` step runs five sections with
  zero tests.

Outside this job entirely, and therefore outside the inventory, are the separate
`offline_cursor` job (`python3 -I -m unittest discover -s experiments/cursor-cloud -p
test_client.py -v`) and the Linux jobs later in `.github/workflows/ci.yml` (for example
the Linux `cargo nextest` job at line 169 and the Linux reporting-load jobs). Their
executions are not part of the frozen job and are not counted.

### 2.4 Shared bodies, instances, and logical checks

`tests/support/live.rs` defines a `#[test]` at lines 567–581
(`live::command_clears_inherited_reporting_authority`, source range 568–579). Because the
file is a shared support module compiled into many integration tests, that one body
executes 28 times, once in each of 28 binaries. The inventory keeps one row per
execution ID (`runner=nextest`, distinct `suite` binary, same `source_file/start/end`)
and the rubric requires exactly that: review the body once, but keep one output row per
execution ID.

Runner rows are not interchangeable, and the columns make the distinction explicit:

- a **nextest** row is a test instance inside one binary (`suite` = binary ID,
  `target_kind` = `lib`/`bin`/`integration`), executed as its own process;
- a **node-test** row is one TAP-reported case (`target_kind=node`, `suite` = `.mjs`
  file), including loop-generated cases where the source range is the loop;
- a **python-unittest** row is one executed `test_` method (`target_kind=unittest`);
- a **python-check** row is one executed check/table-loop element or counter-incrementing
  block (`target_kind=script-check`), not one assertion;
- a **cargo-test** row is one explicit `--exact` Rust test (`target_kind=lib`,
  `suite=ovrcr::lib`), with nested Node cases as children.

## 3. Rubric

The frozen rubric scores four integer dimensions (0–4) and states: "Names, length,
assertion count, passing status, or keyword classification do not justify scores. State
a concrete plausible defect the assertions would detect; if none exists, say why."

### 3.1 Dimension anchors

**C — criticality of protected behavior**

| Score | Anchor |
|---:|---|
| 0 | no identifiable product/test-infrastructure consequence |
| 1 | cosmetic/incidental detail or minor maintenance consequence |
| 2 | bounded feature failure with a straightforward workaround |
| 3 | major workflow failure, unusable terminal/session, lost work, wrong ownership, or durable-state integrity threatened |
| 4 | credential/privacy breach, destructive wrong-target action, irreversible data loss, or violation of the one-server / 50-session guarantees |

**S — regression detection signal**

| Score | Anchor |
|---:|---|
| 0 | tautological, unreachable/vacuous, or no effective assertion |
| 1 | broad smoke check permitting most plausible wrong outcomes |
| 2 | specific happy-path regression caught, important wrong outcomes escape |
| 3 | meaningful boundary/failure/transition assertions that reject plausible defects |
| 4 | precise state-preserving positive/negative assertions, or a decisive complete invariant, that catch the stated subtle defect |

**U — distinct coverage**

| Score | Anchor |
|---:|---|
| 0 | fully subsumed by named retained coverage; no extra boundary |
| 1 | mostly redundant with only a small evidenced distinction |
| 2 | distinct input/boundary within an already-covered contract |
| 3 | distinct failure mode or integration boundary in reviewed coverage |
| 4 | only evidenced protection for an important failure mode after cross-suite comparison |

**F — fidelity to the claimed behavior**

| Score | Anchor |
|---:|---|
| 0 | only source text, a copied algorithm, or a stand-in for the subject |
| 1 | a plumbing check replaces the subject's behavior |
| 2 | real component with a meaningful boundary replaced (state the limit) |
| 3 | production behavior through its relevant API with appropriate external fixtures |
| 4 | the full boundary its claim needs, including real OS/process/storage when that boundary IS the subject |

### 3.2 Derived scores

```
criticality_score = 25*C
usefulness_score  = 10*S + 10*U + 5*F
retention_score   = 10*C + 6*S + 6*U + 3*F
```

The assembler recomputes these; supplied derived values that disagree are rejected. The
rubric is explicit that runtime, observed retries, and maintenance friction never lower
these scores: "Score value only." Cost and flake observations belong in `cost_drivers`
and in the measured-time section, not in the value formula.

### 3.3 Safeguards

- A pure algorithm test can earn F=4 for directly testing the full production algorithm;
  spawning a subprocess does not itself earn F=4.
- An `is_ok()` assertion can protect a historical wire-format contract. Two tests
  touching the same message type are not automatically duplicates.
- Similar negative cases can protect different authority states; compare fixtures and
  branches before assigning low U.
- Generated/table-driven matrices can contain distinct boundaries; judge the real
  assertions and independent expectations, not loop size.
- A body that executes in many binaries is reviewed once per body but keeps one output
  row per execution ID; if contexts are equivalent, the lexicographically smallest full
  `test_id` is the retained representative and the rest are overlap candidates of it.
- Mocks and test-only constructors lower F only when they bypass the claimed behavior. A
  fake provider can legitimately test OVRCR composition but cannot prove native provider
  acceptance.
- Nextest runs each test in a separate process; a process-local mutex does not serialize
  hosted cases.
- Sleeps, polling, subprocesses, PTYs, and Git are observed mechanisms, not measured
  durations or proven causes of flakiness.
- Claim historical regression value only with inspected source or history evidence; an
  unverified comment is not proof.

## 4. Column definitions

The report CSV has 40 columns in this exact order. Identity columns (rows 1–13) must
never disagree between `inventory.csv` and a review record.

| # | Column | Meaning |
|---:|---|---|
| 1 | `test_id` | Unique execution ID. Constructed by percent-encoding and joining `(runner, ci_step/invocation, suite, test_name, variant)` with `|`; the encoding uses `urllib.parse.quote(..., safe="")`. Example: `nextest|Test|ovrcr%3A%3Api_delegation|<module path>|default`. |
| 2 | `parent_test_id` | Empty, or the full `test_id` of the Rust parent whose step spawned this nested Node case; always resolvable in the same file and never self-referential. |
| 3 | `revision` | Pinned revision, one value across the file. |
| 4 | `ci_step` | Workflow step name that produced the row (`Test`, `Extension tests`, `Relocated macOS startup assets`, the two installed-Pi steps, etc.). |
| 5 | `runner` | `nextest`, `cargo-test`, `node-test`, `python-unittest`, or `python-check`. |
| 6 | `suite` | Binary ID (nextest), `ovrcr::lib` (cargo-test), `.mjs` file (node-test), `file::Class` (python-unittest), or script path (python-check). |
| 7 | `target_kind` | `lib`, `bin`, `integration`, `node`, `unittest`, or `script-check`. |
| 8 | `test_level` | Worker-assigned `unit`/`integration`/`acceptance` from the body-based boundary rule, independent of Cargo target kind. |
| 9 | `test_name` | Runner-reported name: module path, TAP case name, unittest method, or python-check label. |
| 10 | `variant` | `default`, `production`, or `local-development`: which invocation/profile produced the row. |
| 11 | `source_file` | Repository-relative source file (test file, `.mjs`, or check script). |
| 12 | `source_start` | First line of the test/loop/block body, 1-based. |
| 13 | `source_end` | Last line of that body, inclusive. |
| 14 | `production_refs` | JSON array of `path:start-end` references to the production behavior the assertions protect. |
| 15 | `behavior` | Concrete description of what the test asserts, not a name paraphrase. |
| 16 | `failure_caught` | Concrete plausible defect the assertions would reject. |
| 17 | `criticality` | C score, 0–4. |
| 18 | `signal` | S score, 0–4. |
| 19 | `uniqueness` | U score, 0–4. |
| 20 | `fidelity` | F score, 0–4. |
| 21 | `criticality_score` | Recomputed `25*C`. |
| 22 | `usefulness_score` | Recomputed `10*S + 10*U + 5*F`. |
| 23 | `retention_score` | Recomputed `10*C + 6*S + 6*U + 3*F`. |
| 24 | `criticality_reason` | C rationale with evidence citation. |
| 25 | `signal_reason` | S rationale with evidence citation. |
| 26 | `uniqueness_reason` | U rationale naming compared coverage. |
| 27 | `fidelity_reason` | F rationale naming the real boundary or its limit. |
| 28 | `confidence` | `high`, `medium`, or `low` under the rubric definition. |
| 29 | `recommendation` | `investigate`, `consolidate`, `prune_candidate`, `improve`, or `keep`. |
| 30 | `recommendation_reason` | Gate-specific rationale. |
| 31 | `overlap_test_ids` | JSON array of full `test_id`s for confirmed overlaps; each resolves and none names its own row. |
| 32 | `cost_drivers` | Observed mechanisms only (PTY, subprocess, polling, SQLite, Git, retries); a string, never a score input. |
| 33 | `first_attempt_seconds` | Duration of attempt 1 as measured in this run; measured seconds as text. |
| 34 | `total_attempt_seconds` | Sum of all attempts for the execution; equals the first attempt for a single run. |
| 35 | `attempt_count` | Observed number of attempts. |
| 36 | `failed_attempt_count` | Observed failed attempts. |
| 37 | `timing_basis` | `hosted-single-run` for measured rows; `unmeasured` when the runner prints no per-case timing. |
| 38 | `timing_source` | Provenance: log line/job/run, or the reason a timing is unavailable. |
| 39 | `reviewer` | Authoring agent ID. |
| 40 | `second_reviewer` | Independent reviewer ID from a quality-review receipt; empty when none. |

Additional column rules:

- **ID construction.** The parts are `runner`, invocation/step, suite, name, and variant;
  each is individually percent-encoded so `:` and `/` are escaped (`ovrcr::pi_delegation`
  becomes `ovrcr%3A%3Api_delegation`). The tuple is unique across the file (3171 distinct
  IDs). A nextest row uses `Test` as the invocation; a cargo-test row uses the step name
  and `production`; a node batch row uses `Extension tests`; the two nested Node files use
  their parent step name and `production`.
- **Variant values.** `default` is the ordinary invocation; `production` is the
  `--exact --ignored` Pi parent/nested run; `local-development` is the second packaging
  profile run by `scripts/package-startup.sh --local-development` and the corresponding
  `--local-development` checks.
- **Timing columns.** `first_attempt_seconds` is the first attempt, not cumulative;
  `total_attempt_seconds` is cumulative across attempts. A test with two attempts has
  `attempt_count=2`, `failed_attempt_count=1`, and first/total differing. Cumulative
  test-process seconds are not wall-clock: nextest runs test processes concurrently, the
  Node files in the batch run concurrently, and nested TAP durations run inside their
  parent step. Unknown timings are empty strings, never `0`: 239 rows
  (`39 python-unittest + 200 python-check`) are `unmeasured`.
- **Array cells.** `production_refs` and `overlap_test_ids` are native JSON arrays,
  serialized into the CSV cell with `json.dumps(list, ensure_ascii=False)` and requiring
  exact round-trip.
- **Ordering.** Rows are sorted by `(retention_score, test_id)` ascending; the writer
  must reproduce that order and the reader must parse it back cell-for-cell.

## 5. Gates

The recommendation policy is ordered and first match wins:

1. **investigate** — unresolved behavioral evidence, low confidence, or contested
   overlap; specify the precise boundary and the next verification. Never a label for a
   test that was not read.
2. **consolidate** — high-confidence, independently confirmed redundant coverage or
   repeated execution with existing named retained protection; for C ≥ 3 explicitly
   document preservation of the high-criticality invariant.
3. **prune_candidate** — high-confidence, C ≤ 2, usefulness ≤ 25, and either no effective
   behavioral protection or demonstrated complete subsumption by retained tests.
4. **improve** — S ≤ 1 or F ≤ 1 while meaningful behavior needs protection (C ≥ 2 or
   U ≥ 2), with no proven equivalent coverage; name the missing assertion or production
   path.
5. **keep** — remaining grounded coverage.

The rubric adds: "Do not force a pruning quota. Cost and flake concerns are reported in
`cost_drivers` and never turn a valuable test into a prune_candidate."

**Confidence.** `high` = body + production behavior + any overlap claim directly checked;
`medium` = behavior understood with a stated boundary/comparison uncertainty; `low` =
material behavioral inference remains.

**Independent second review.** A quality-review receipt is required for every row with
`criticality == 4`, every `prune_candidate` or `consolidate` row, and every `low`-confidence
row. A receipt names a `second_reviewer` different from the worker `reviewer`, a verdict
of `agree` (with `final_review: null`) or `resolved` (with a non-null adjudicated record),
and non-empty findings. Duplicate receipts, receipts for unknown rows, empty reviewer IDs,
and self-review fail validation. The CSV `second_reviewer` column is empty when no receipt
exists.

**Overlap integrity.** A confirmed overlap group has a unique `group_id`, unique members,
a non-empty subset of retained IDs, non-empty preservation evidence, and at least one
retained member that is not itself a removal candidate. Removal recommendations must
reach a surviving protector; a two-way "A retained only by B, B only by A" chain fails.
Every `overlap_test_ids` entry must resolve and must not name its own row.

## 6. Measured job time

All times are from the single frozen hosted run. They describe this run; they are not a
benchmark or a distribution.

| Measurement | Value | Basis |
|---|---:|---|
| Job wall time | 2415 s = 40 m 15 s | first step start `00:27:42Z` to last step completion `01:07:57Z` |
| Sum of 23 step durations | 2415 s | `job-steps.csv` duration column |
| Test step | 2154 s | `Test` step start `00:28:43Z`, end `01:04:37Z` |
| Compilation inside Test | 136 s = 2 m 16 s | `Finished test profile … in 2 m 16 s` |
| Nextest execution | 2014.918 s | nextest Summary |
| Test-step residual | 3.082 s | 2154 − 136 − 2014.918 |
| Cumulative test-process time | 5931.968 s | sum of all 2822 attempt durations |

`run.json` records the job as started at `2026-11-08T00:27:42Z` and completed at
`2026-11-08T01:08:00Z`; the step ledger's last completion is `01:07:57Z`. The difference
is the reporting boundary between the final step and the job record, not test work.

### 6.1 Cumulative test-process time by binary (top groups)

`attempts.csv` contains 2822 attempt records over 2820 unique `(binary_id, test_name)`
pairs in 56 binaries. Summing each attempt's `seconds` by binary gives:

| Binary | Test instances | Process seconds |
|---|---:|---:|
| `ovrcr::pi_delegation` | 260 | 2380.841 |
| `ovrcr::server_lifecycle` | 157 | 764.781 |
| `ovrcr::cursor_cloud` | 47 | 702.799 |
| `ovrcr::automatic_titles` | 34 | 307.713 |
| `ovrcr::retained_sessions` | 22 | 219.666 |
| `ovrcr::claude_quota_probe` | 14 | 167.450 |
| `ovrcr-runtime` | 496 | 154.868 |
| `ovrcr::terminal_acceptance` | 34 | 154.485 |
| `ovrcr::provider_quota` | 33 | 134.734 |
| `ovrcr::gui` | 13 | 99.716 |

The remaining binaries account for the rest of the 5931.968 s.

### 6.2 Recovered retries

Two executions failed once and passed on the next attempt. Both are reported as FLAKY in
the nextest summary, and both keep `attempt_count=2`, `failed_attempt_count=1`:

| Test | Attempt 1 | Attempt 2 | Total | Attempts | Failed |
|---|---:|---:|---:|---:|---:|
| `ovrcr::codex_initial_prompt::codex_harness_default_and_explicit_prompt_use_the_shared_lifecycle_and_privacy_path` | 4.855 s FAIL | 4.372 s PASS | 9.227 s | 2 | 1 |
| `ovrcr::pi_delegation::parent_recovery::parent_recovery_transfer_discovers_prepared_and_suspends_accepted_unknown_partial_effects` | 24.507 s FAIL | 21.743 s PASS | 46.250 s | 2 | 1 |

### 6.3 Caveats

- **Concurrent durations do not sum to wall-clock savings.** Nextest executes test
  processes concurrently, the nine Node files in the batch run concurrently, and nested
  Node durations run inside their parent Rust step. The 5931.968 s of test-process time
  is a sum of observed process/attempt durations, not elapsed time, and it cannot be
  compared one-for-one with the 2415 s job wall time.
- **Parent and nested durations overlap.** The two `cargo test` parents report 5.37 s and
  7.25 s, while their nested TAP streams report 1492.879958 ms and 1904.338042 ms for the
  children. The child durations are contained inside the parent steps and must not be
  added to the parent duration as if they were sequential work.
- **This is a single hosted run.** It establishes no flake rate, no percentile, and no
  variance. The two retries are observations of this run, not a measured failure
  probability.
- **No CI speedup is established.** The measurements describe the current job; they do
  not compare an alternative selection, sharding, or build strategy. Any speedup claim
  would require a controlled A/B run with the same revision and runner.
- **Unmeasured rows are not zero.** The 239 Python rows have empty timing cells because
  those runners print no per-case timing (unittest prints one aggregate per class; the
  `check-*.py` scripts print counters). Treating them as zero would understate work.

## Numbers and sources

| Claim / number | Source |
|---|---|
| Pinned revision, run ID, job ID, URL, conclusion, head SHA, timestamps | `evidence/manifest.json:2-15`; `run.json:1` |
| Retrieval commands and retrieval times | `evidence/manifest.json:17-27` |
| File byte sizes and SHA-256 values | `evidence/manifest.json:31-88` |
| Unmodified tracked tree (statement) | `git rev-parse HEAD`; `git status --porcelain` at the pinned revision |
| macOS job command set (format, lint, nextest, doctests, package checks, Cursor, node, two Pi steps) | `.github/workflows/ci.yml:41,44,48,50,53-63,66-69,74,86,94`; `evidence/job-steps.csv:2-24` |
| 3171 rows; runner split 2820/2/110/39/200 | `evidence/inventory.csv:2-3172` (aggregation); `evidence/manifest.json:148-181` |
| 2820 tests, 56 binaries, 33 skipped | `evidence/macos-job.log:505` |
| 2853 compiled = 2820 + 32 ignored + 1 filtered | `evidence/exclusions.csv:2` |
| 4 cfg(not macOS) tests excluded | `evidence/exclusions.csv:4-6,32` |
| 32 source `#[ignore]` rows | `evidence/exclusions.csv:3-38` |
| feature-gate scope note | `evidence/exclusions.csv:39` |
| `pi_publish_contract_installed.mjs` not invoked | `evidence/exclusions.csv:40`; `.github/workflows/ci.yml:74` |
| five doctest sections, 0 tests each | `evidence/exclusions.csv:41`; `evidence/macos-job.log:3414-3440` |
| 106 TAP cases and per-file split | `evidence/node-notes.md:3-40`; `evidence/macos-job.log:4338-4346` |
| 4 nested Node rows, parent IDs, TAP ranges, durations | `evidence/node-notes.md:60-90`; `evidence/macos-job.log:4379-4481` |
| 39 unittest methods and 200 logical checks; unmeasured basis | `evidence/python-checks.md:1-40`; `evidence/macos-job.log:3631,3669` |
| 28 executions of the shared fixture body | `tests/support/live.rs:567-581`; `evidence/inventory.csv:2-3172` grouping by `source_file/source_start/source_end` |
| ID construction rule | `evidence/build_inventory.py:623-626` |
| Rubric anchors, formulas, safeguards, ordered gates, confidence | `evidence/rubric.md:3-57` |
| CSV header order, array serialization, row ordering | `evidence/schema.md:140-152` |
| Review field requirements and receipt rules | `evidence/schema.md:79-140` |
| Job wall 2415 s; Test 2154 s; Doc tests 49 s; packaging/Node/Pi step durations | `evidence/job-steps.csv:11,12,16,18,19,24` |
| Compilation 136 s and nextest execution 2014.918 s | `evidence/macos-job.log:502,3396`; `evidence/job-steps.csv:11` (notes column) |
| 2822 attempts, 2820 unique pairs, 56 binaries, 5931.968 s cumulative | `evidence/attempts.csv:2-2823` (aggregation); `evidence/manifest.json:98-120` |
| Per-binary cost groups | `evidence/attempts.csv:2-2823` (sum by `binary_id`) |
| Codex retry 4.855/4.372/9.227/2/1 | `evidence/attempts.csv:342,345`; `evidence/manifest.json:121-130` |
| Pi retry 24.507/21.743/46.250/2/1 | `evidence/attempts.csv:717,720`; `evidence/manifest.json:131-140` |
| FLAKY summary and two retried names | `evidence/macos-job.log:3396-3398` |
| Parent step runtimes 5.37 s / 7.25 s; nested durations | `evidence/macos-job.log:4428,4481`; `evidence/node-notes.md:70-90` |

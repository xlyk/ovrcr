# macOS test value review

## Context

Review **all tests in the GitHub Actions job named `macOS checks`**, with explicit unit/integration/acceptance labels, and deliver a per-test CSV that explains usefulness and criticality for later pruning. The user reports a full run exceeding 30 minutes and requests a multi-agent division of work. This task evaluates tests; it does not delete tests, weaken assertions, or change CI selection.

## Findings established during planning

- Planning checkout is clean on `chore/unit-test-review`, revision `992a1da5c59a865ebb48051e4d9d8ff7184c8dc5`.
- `.github/workflows/ci.yml:18-102` defines the named job. Its main Rust command is `cargo nextest run --workspace --all-targets --all-features --profile ci -E 'not test(partial_resize_connection_delivers_error_before_owner_close)'`. It separately runs default-feature doctests, macOS packaging/installer checks, nine Node test files, and two exact ignored installed-Pi integration tests.
- `.config/nextest.toml:7-22` sets two retries, fail-fast, a five-minute timeout, and exclusive nextest scheduling for four lifecycle tests. Retries and scheduling must be represented separately from test value.
- `.github/workflows/ci.yml:104-115` puts offline Cursor preflight in a different job. Do not conflate it with the selected job.
- `scripts/ci-tests.sh:13-21` is an older coarse runner, not the command invoked by this job. Inventory must follow the workflow rather than this script.
- `Cargo.toml:1-10,60-67` establishes five workspace packages, Rust minimum 1.97.1, and GUI plus acceptance-diagnostics features selected by the main macOS command.
- `docs/development/testing.md:3-12,14-55` requires real behavioral assertions, deterministic race evidence, isolated fixtures, and honest evidence boundaries. `Live` fixtures own private resources; thread-hosted adapters share process environment. The settings matrix generates cases inside tests.
- `docs/development/architecture.md:3-9` provides the natural subsystem split: protocol, terminal, runtime, TUI, and root composition/CLI/reporting.
- A successful hosted run matches this exact revision: run `42429838269`, job `128576086691`, https://github.com/xlyk/ovrcr/actions/runs/42429838269/job/128576086691. Job duration was 40m18s. Its main Test step took 35m54s; the log separates about 2m16s of compilation from nextest's 2014.918s execution.
- The matching hosted log reports 2820 Rust tests across 56 binaries, 33 skipped, 2820 passed including two recovered retries and three slow cases. The ordinary Node batch reports 106 tests; each installed-Pi parent runs two nested Node cases. All five package doctest suites report zero tests. These are baseline observations, not estimates or a promise of future counts.
- The hosted nextest version was 0.9.151. `cargo nextest` is not available on the planning shell's PATH. Existing evidence under `research/` has older revisions and must not be mixed into current timing statistics.
- Scoring will keep criticality, regression signal, distinct coverage, and production-path fidelity separate. Runtime and observed retries will not lower the value score automatically.
- Source-level pitfalls: `crates/ovrcr-runtime/tests/delegation_fallback_files.rs:5-8` includes production modules with inline tests, so the same body may execute in multiple binaries; `crates/ovrcr-runtime/tests/settings_matrix.rs:576-641` loops over settings inside individually named tests. Count runner cases, not assertion calls or `#[test]` strings.
- `scripts/package-startup.sh:70` invokes the normal build path in `scripts/build-bridge.sh`; its Swift headless runners are behind `--check` (`build-bridge.sh:50-68`) and are not invoked by these packaging steps. Keep ordinary build-time validation separate from test cases.

## Approach

### 1. Freeze evidence and inventory before scoring

Work from the repository root on the pinned revision above. Create the requested report at `research/macos-test-review/tests.csv`, its scoring guide and findings at `research/macos-test-review/methodology.md`, and supporting artifacts below that directory. The coordinator owns final artifacts. These are new report files; no reusable test-review generator was found in the examined scripts, workflows, or research evidence. Build only a report-local assembler, not a general test-management framework.

1. Fetch `gh run view 42429838269 --json headSha,jobs,url,conclusion` and `gh run view 42429838269 --job 128576086691 --log`. Save as `evidence/run.json` and `evidence/macos-job.log`; record command, retrieval time, revision, and file SHA-256 hashes in `evidence/manifest.json`. Confirm the run names the pinned SHA.
2. Parse the main nextest invocation's terminal attempt records into `evidence/attempts.csv`: invocation, binary ID, full test name, attempt number, result, seconds, raw log line. Strip both actual ANSI sequences and displayed `^[[...m` escapes. Ignore `SLOW` progress and repeated final `FLAKY` summaries. Observed baseline: **2822 attempt records, 2820 unique executed tests, 56 binaries, 2 failed attempts, 5931.968 cumulative test-process seconds**. The last figure is not wall time.
3. Build `inventory.csv` from executed IDs. Map each to its complete definition, attributes, and source range by reading source/module declarations. Use full binary ID plus module-qualified name, never basename alone. Account for `#[path]`, platform/features, ignored tests, and shared inline tests. Regex attribute counts are not authoritative. The complete matching hosted run avoids a new full-suite execution.
4. Add the two explicit ignored Rust invocations from workflow lines 86 and 94: `production_materializer_loads_local_package_with_one_injected_owner` in `ovrcr::pi_package`, and `coding::production_assignment_argv_preserves_installed_sdk_child_publication_tools` in `ovrcr::pi_delegation`. They are absent from the main 2820. Their separately reported single-test run durations are 5.37s and 7.25s; compilation/npm time is separate.
5. Expand the 106 Node names from the workflow's nine-file batch and join their TAP results/durations. Include the two cases in `tests/pi_package_installed.mjs` and two in `tests/pi_child_tools_installed.mjs` as child rows linked to their Rust parent. These four cases have distinct reviews but their duration is already included in their parent. Keep assertion loops inside their named runner case. `tests/pi_publish_contract_installed.mjs` is not invoked by this job and stays outside the scored inventory.
6. Enumerate 33 `InstallerTests` methods in `native/bridge/check-installer.py` and six methods in `tests/cursor_bridge_install.py`. For sequential Python checks, enumerate each executed `check()`/`validate()` invocation, loop-generated case, or standalone block ending in a check-counter increment, rather than each assertion. Record mode, input label, and source anchor. Hosted totals: `check-local-startup.py` 17; `check-startup-package.py` production 12/local 28; `check-packaging.py` production 27/local 36; `check-guards.py` production 40/local 40. Reconcile source mapping to these totals. Never import top-level scripts to enumerate them: imports execute their mutations.
7. Record the five zero-test doctest results without inventing rows. Put format/lint, compilation, package construction, provisioning, upload, and build-time validation in `evidence/job-steps.csv` with step times/classification. The normal package build does not run the Swift `--check` runners. Exclude the separate offline Cursor and manual Linux jobs.
8. Record ignored/filter/platform exclusions in `evidence/exclusions.csv`, outside the scored inventory. The main log reports 33 skipped without naming them; reconcile source attributes and the filter rather than inventing identities. Two ignored tests re-enter through the explicit Pi invocations. Absent tests never receive zero scores.

Expected scored inventory at this revision: **3171 rows** = 2820 ordinary Rust + 2 explicit Rust + 106 ordinary Node + 4 nested Node + 39 unittest methods + 200 sequential script checks. Runner test instances and logical script checks deliberately remain distinguishable through the CSV's type fields. A mismatch is a reconciliation failure to resolve, not permission to change totals until green.

### 2. Apply one anchored rubric

For every test, read its full body, assertion helpers, relevant fixture behavior, production code, and neighboring coverage. Names, length, assertion count, passing status, or keyword classification do not justify scores. State a concrete plausible defect the assertions would detect. If none exists, explain why.

Score four dimensions with integers 0–4:

|Score|C: criticality of protected behavior|S: regression detection signal|U: distinct coverage|F: fidelity to the claimed behavior|
|---|---|---|---|---|
|0|No identifiable product/test-infrastructure consequence|Tautological, unreachable/vacuous, or no effective assertion|Fully subsumed by named retained coverage; no extra boundary|Only source text, copied algorithm, or a stand-in for the subject|
|1|Cosmetic/incidental detail or minor maintenance consequence|Broad smoke check permits most plausible wrong outcomes|Mostly redundant with only a small evidenced distinction|Plumbing check replaces the subject's behavior|
|2|Bounded feature failure with straightforward workaround|Specific happy-path regression caught; important wrong outcomes escape|Distinct input/boundary within an already-covered contract|Real component with a meaningful boundary replaced; state the limit|
|3|Major workflow failure, unusable terminal/session, lost work, wrong ownership, or durable-state integrity threatened|Meaningful boundary/failure/transition assertions reject plausible defects|Distinct failure mode or integration boundary in reviewed coverage|Production behavior through its relevant API with appropriate external fixtures|
|4|Credential/privacy breach, destructive wrong-target action, irreversible data loss, or violation of one-server / 50-session guarantees|Precise state-preserving/positive/negative assertions, or decisive complete invariant, catch the stated subtle defect|Only evidenced protection for an important failure mode after cross-suite comparison|Full boundary needed for its claim, including real OS/process/storage when that boundary is the subject|

The coordinator calculates:

- `criticality_score = 25*C`.
- `usefulness_score = 10*S + 10*U + 5*F`.
- `retention_score = 10*C + 6*S + 6*U + 3*F`.

Each derived score ranges 0–100; higher means more reason to retain. These are review judgments, not probabilities. Keep components/reasons visible. Runtime, retries, and maintenance friction do not lower the value formula.

Apply these safeguards:

- A pure algorithm test can earn F=4 for directly testing the full production algorithm. Spawning a process does not itself earn F=4.
- `is_ok()` can protect a historical wire-format contract. `codec.rs:463` tests JSON deserialization; `agent.rs:1073` uses binary framing. Do not label them duplicates just because both touch reservation messages.
- Similar negative cases can protect different authority states. `desktop.rs:2072,2083` differ in sample quality. Compare fixtures and branches before assigning low U.
- Generated matrices can contain distinct boundaries. Assess each named settings test's real assertions and independent expectations, not loop size.
- The `tests/support/live.rs:568` body executes in 28 main-run binaries. Review its body once and preserve all execution rows. Compare compilation/fixture context before judging repeated executions redundant. If contexts are equivalent, select the lexicographically smallest full ID as the retained representative.
- Mocks/test-only constructors lower fidelity only when they bypass the claimed behavior. A fake provider can legitimately test OVRCR composition, but cannot prove native provider acceptance.
- Nextest runs each test in a separate process. `tests/server_lifecycle.rs:32-46`'s process-local mutex does **not** serialize hosted nextest cases. Only record actual runner-level scheduling constraints as such.
- Sleeps, polling, subprocesses, PTYs/Git, and cleanup are observed mechanisms, not measured durations or proven causes of flakiness.
- Claim historical regression value only with inspected source/history evidence; an unverified comment is not proof.

### 3. Calibrate and fan out bounded review assignments

The coordinator owns inventory, timing, formulas, merging, and final adjudication. Workers only read product/test source and write their exclusive report fragments. They must skip builds/lint/tests/formatters and never modify tests. Every brief includes this rubric/contract, frozen inventory IDs, full source paths, workflow scope, and the relevant project guides. No recursive delegation.

First two independent reviewers score these same 12 calibration cases:

1. Protocol JSON reservation at `codec.rs:463`.
2. Protocol binary roundtrip/redaction at `agent.rs:1073`.
3. Runtime settings `explicit_non_default_takes_effect`.
4–5. Desktop no-unread cases at `desktop.rs:2072,2083`.
6. Shared fixture case at `tests/support/live.rs:568`.
7–9. First executed full ID, sorted lexically, in each of `ovrcr::pi_delegation`, `ovrcr::terminal_acceptance`, `ovrcr::claude_quota_probe`.
10. Node `a late startup reply cannot restore a retired factory`.
11. Production packaging `callback_hash` case.
12. Production compiled-guard open-stdin timeout at `check-guards.py:77-87`.

The coordinator resolves recommendation disagreements and component differences >=2 by re-reading evidence. Save agreed scores/reasons in `evidence/calibration.jsonl`. Clarify anchors consistently; do not alter weights per subsystem.

Then generate exact ID manifests and launch independent review tasks in concurrent batches of up to 12. Start queued shards as slots become available. Use these first-match workstreams:

|Workstream|Exclusive definition ownership|
|---|---|
|Shared fixtures|`tests/support/**`, including repeated execution instances|
|Protocol and terminal|`crates/ovrcr-protocol/**`, `crates/ovrcr-terminal/**`|
|Runtime delegation|`crates/ovrcr-runtime/src/server/delegation*` and descendants; crate integration files `delegation_*`|
|Runtime server/session|Remaining runtime `src/server/**`, `src/session/**`|
|Runtime storage/settings/tasks|Remaining `crates/ovrcr-runtime/**`|
|TUI library|`crates/ovrcr-tui/**`, including its integration targets|
|Application library/bin|Root `src/**`, grouped by report/operator/gui/cli/remote, then remaining files|
|Public delegation|`tests/pi_delegation.rs`, `tests/pi_delegation/**`|
|Lifecycle/terminal acceptance|`tests/server_lifecycle.rs`, `tests/terminal_acceptance.rs`, `tests/terminal_acceptance/**`|
|Public Dashboard|`tests/tui/**`|
|Other Rust integration|Every remaining root Rust test definition, grouped by binary|
|Extensions/installed Pi|Selected `.mjs` definitions including nested cases|
|Packaging/installers|Selected Python methods/check blocks, grouped by script and mode|

Within a workstream, group by `(definition path, start line)` so repeated bodies stay together. Sort by path/line; greedily pack <=80 unique definitions and <=8000 distinct test-body lines per shard. An oversized individual body stays intact. Split large files only between whole test/check bodies. Save exact assignments in `evidence/assignments.json`; each body belongs to one shard. Supporting helpers can be read by all workers.

Workers write `reviews/<shard-id>.jsonl`, one record per assigned execution ID. Repeated bodies may share grounded analysis, but uniqueness/recommendation must account for execution context. Each record contains all semantic fields from the CSV contract, including body/assertion citations, production references, a concrete defect, four scores/reasons, confidence, proposed recommendation, and full-ID overlap candidates. No filename-based scoring, blanket boilerplate, or unread rows.

While workers review, the coordinator completes timing/source joins and the assembler. Search the full inventory for competing coverage before confirming overlap. Unresolved source/identity mapping blocks inventory completion; it does not make a test low-value.

### 4. Reconcile overlap and independently check risky recommendations

Build `evidence/overlap-groups.json` as an array of objects with `group_id`, `member_test_ids`, `retained_test_ids`, and `preservation_evidence` (assertion/source references and boundary comparison). Shared setup/names are insufficient. A confirmed redundant group preserves the same input/failure, path, assertions, and configuration boundary in existing retained tests. It cannot remove all protection or use another removal/consolidation candidate as its sole survivor. Distinct cases requiring a future replacement are not confirmed redundancy: retain them and describe possible future consolidation in the recommendation reason.

Assign independent agents, in parallel, to review all proposed `prune_candidate`/`consolidate` rows, all C=4 rows, all low-confidence rows, and the 30 slowest measured top-level rows. Also check 10% of every remaining shard: first `ceil(0.10*n)` IDs sorted by SHA-256 of full ID. Reviewers must not author the checked rows. They read source and counterparts, not just CSV prose. Save author/reviewer IDs, findings, and resolutions in `evidence/quality-review.jsonl`. The coordinator adjudicates with evidence, never averages scores.

Use this ordered recommendation policy:

1. `investigate`: unresolved behavioral evidence, low confidence, or contested overlap. Specify the precise boundary and next verification. This is a completed finding, never a label for an unread test.
2. `consolidate`: high-confidence independently confirmed redundant coverage or repeated execution, with existing named retained protection. For C>=3 explicitly document preservation of the high-criticality invariant. Do not label distinct scenarios redundant just because a future parameterized replacement could combine them.
3. `prune_candidate`: high-confidence independently checked, C<=2, usefulness<=25, and either no effective behavioral protection or demonstrated complete subsumption by retained tests. Duplicates fitting rule 2 stay `consolidate`. No automatic deletion.
4. `improve`: S<=1 or F<=1 while meaningful behavior needs protection (C>=2 or U>=2), without proven equivalent coverage. Name the missing assertion/production path.
5. `keep`: remaining grounded coverage. Cost/flake concerns stay separate and may suggest optimization, not deletion.

Confidence: `high` means body, production behavior, and any overlap claim directly checked; `medium` means behavior understood with a stated boundary/comparison uncertainty; `low` means material behavioral inference remains. Missing timing alone does not lower semantic confidence. Do not force a pruning quota.

### 5. Assemble the CSV and reading guide

Write UTF-8 RFC 4180 CSV with Python's standard `csv` module. Arrays are JSON arrays within quoted cells; unknown numeric measurements are empty, never zero. Preserve commas/quotes/Unicode/newlines.

Exact header/order:

```text
test_id,parent_test_id,revision,ci_step,runner,suite,target_kind,test_level,test_name,variant,source_file,source_start,source_end,production_refs,behavior,failure_caught,criticality,signal,uniqueness,fidelity,criticality_score,usefulness_score,retention_score,criticality_reason,signal_reason,uniqueness_reason,fidelity_reason,confidence,recommendation,recommendation_reason,overlap_test_ids,cost_drivers,first_attempt_seconds,total_attempt_seconds,attempt_count,failed_attempt_count,timing_basis,timing_source,reviewer,second_reviewer
```

- ID: join percent-encoded `(runner, invocation, suite, full test_name, variant)` with `|`; UTF-8 URL encoding with `safe=""`. Invocation is exact workflow step name. Repeated packaging commands differ by suite/mode. IDs are stable within this frozen revision.
- `runner`: `nextest|cargo-test|node-test|python-unittest|python-check`. `suite`: full nextest binary ID, Node path, Python path/class, or sequential script path. `variant`: `default|production|local-development`, with input-case labels as needed. Unnamed script blocks use descriptive names plus source line for disambiguation; freeze their mapping.
- `parent_test_id`: only the four installed-Pi children; parent must exist in the same step.
- `target_kind`: `lib|bin|integration|node|unittest|script-check|doctest`. Derive Rust kind from manifest/module roots.
- `test_level`: `unit` for isolated function/module/model; `integration` for multiple OVRCR components with controlled external dependencies; `acceptance` for compiled CLI/Dashboard/server or packaged artifact through a real external boundary. Keep this independent of Cargo target kind.
- Source: repository-relative path and inclusive defining-body range. `production_refs`: JSON array of inspected path:line-range references. Fixture-only cases cite protected fixture code and state this in `behavior`.
- `behavior`/`failure_caught`: concrete descriptions, not name paraphrases. Four score reasons cite evidence. If no effective assertion exists, say so.
- `overlap_test_ids`: resolvable IDs or empty array. `recommendation_reason` distinguishes confirmed redundancy from unproven opportunities to combine distinct scenarios later.
- `cost_drivers`: evidenced observations with source references, such as fixed sleep duration, PTY/Git/process fixture, actual exclusive nextest scheduling, or repeated body. Separate observed failure from inferred flake risk.
- Nextest durations: first attempt and sum of all attempts, including failures. TAP: `duration_ms/1000`. Explicit single-test Cargo parents: reported test-run duration. Parent totals include nested cases.
- Python method/block timings are unknown unless individually reported. Do not divide suite duration or infer method time from buffered timestamps. Preserve suite timing in job evidence.
- `timing_basis`: `hosted-single-run|local-single-run|unmeasured`; `timing_source` identifies file/lines, run/job, and nested-duration inclusion. Unknown timing has empty duration fields.
- Attempt counts reflect observed executions, not population flake rates. If only aggregate script completion is known, leave per-check counts empty and cite aggregate evidence.
- `reviewer` is the authoring agent; `second_reviewer` identifies independent review when required. Quality-review receipts record coordinator adjudication.

Assembler input contract: `inventory.csv` uses the identity/source columns from the final header (`test_id` through `source_end`, except `test_level`) plus the six timing/count/basis/source columns. Fill those timing columns from raw evidence for every runner before assembly; the Rust attempt ledger remains the independent reconciliation source. Workers supply `test_id`, `test_level`, all semantic fields from `production_refs` through `cost_drivers` except the three derived scores, and `reviewer`. Worker arrays are native JSON arrays, scores are integers, and optional values are null. The assembler joins only on `test_id` and rejects conflicting identity fields.

Independent receipts use `{test_id, second_reviewer, verdict, findings, final_review}` per JSONL row, where `verdict` is `agree` or `resolved`; unresolved reviews block assembly. `final_review` is null for agreement or the coordinator-adjudicated complete semantic record for a resolution, with reasoning in `findings`. Preserve original worker records. `assignments.json` is an array of `{shard_id, test_ids}`. Source citations within reason text use `path:start-end`. These small schemas are report-local, not new product APIs.

Sort final CSV by `(retention_score, test_id)` ascending. Keep inventory independently ordered by identity.

The methodology includes frozen revision/CI link, scope/count reconciliation, rubric/formulas, column definitions, confidence/recommendation gates, and measured job-time breakdown. Summarize counts by level/recommendation, largest measured cost groups, overlap groups, and reviewed candidates referencing IDs. Distinguish expensive valuable tests worth optimizing from weak tests worth pruning later. Concurrent process durations and nested durations do not sum to wall-clock savings. Claim neither a CI speedup nor a flake rate from this single run.

## Critical files and anchors

- `tests/support/live.rs:567-578`: 28 executions of one fixture assertion.
- `crates/ovrcr-runtime/tests/delegation_fallback_files.rs:5-8`: path inclusions execute production-module tests in a second binary.
- `tests/pi_delegation.rs:2849-2857`: EOF module declarations defeat header-only mapping.
- `native/bridge/check-guards.py:38-150`: calls, loops, timed blocks, profile branches define logical cases.
- `tests/server_lifecycle.rs:32-46`: a process-local lock is not nextest-wide serialization.

## Verification

Verify the actual report-generation path, not product behavior. Do not rerun the full suite to reconfirm the reported slowness or add permanent product tests for a review.

Keep a report-local standard-library assembler at `evidence/assemble.py`. It reads frozen inventory/attempt records, worker JSONL, overlap decisions, and independent-review receipts; computes scores; writes the CSV; validates it. From repository root:

```sh
python3 -I research/macos-test-review/evidence/assemble.py
python3 -I research/macos-test-review/evidence/assemble.py --check
```

`--check` is read-only and exits nonzero with offending IDs on a structural failure or a failed available-evidence assertion. Normal mode writes `evidence/validation.json` with revision, input hashes, command, status, counts, and per-check `passed|failed|unverified` results. Missing/extra/duplicate reviews fail; never auto-fill scores. If hosted evidence has become unavailable after both retrieval attempts, keep every structural/source/review check mandatory, record only the unavailable hosted-runtime assertions as `unverified` with reasons, leave unsupported measurements empty, and print `complete-with-evidence-gaps` rather than `complete`. Exit zero in that mode means structural validation passed, not that missing hosted checks passed. Whenever the log is available, every exact hosted assertion below remains mandatory.

Required proof:

1. Actual hosted log yields 2820 unique Rust IDs/2822 attempts. Codex initial-prompt retry: first=4.855s, total=9.227s, attempts=2, failed=1. Pi `parent_recovery::parent_recovery_transfer_discovers_prepared_and_suspends_accepted_unknown_partial_effects`: first=24.507s, total=46.250s, attempts=2, failed=1. Final FLAKY summaries add nothing. Total=5931.968s within 0.001s.
2. Exact inventory/CSV ID equality: 3171 rows with the runner/step/mode counts above. Four nested children link to two explicit ignored parents. Five zero-doctest packages produce no rows. Excluded partial-resize test is absent. Under the unavailable-log contingency, preserve the same source/listing identity and review completeness checks; distinguish source-confirmed selection from unverified hosted execution instead of lowering the expected scope.
3. Preserve 28 distinct shared-fixture execution IDs with the same defining source. Internal settings loops create no invented test instances.
4. Every source range exists and matches its definition or documented generated call. Scores are integers 0..4; formulas recompute; all rationale fields are populated; no placeholder/unread/default-scored rows.
5. Parent/overlap references resolve; no self-parent/self-overlap, cycles of sole-survivor recommendations, or groups removing every protector. All required independent reviews and recommendation gates hold.
6. CSV read/write round-trip preserves all cell values. Two assemblies from the same evidence produce identical CSV hashes.
7. Exercise validation on disposable copies/in-memory fixtures: duplicate ID, missing review, invalid score/formula, dangling overlap, absent required second review, and circular sole-survivor references each fail naming relevant IDs. Check quoted commas, Unicode, and newlines round-trip. No source mutation is needed.
8. Read the finished CSV and inspect a real row from every runner plus both retries. Check the methodology's claims against generated receipts. Deliver CSV link, verified row count/findings, and timing/coverage limitations.

## Assumptions and contingencies

- Scope is all tests in `macOS checks`, with unit tests filterable; separate Cursor and Linux jobs stay excluded. No test deletions, assertion changes, or CI changes.
- Analyze pinned `992a1da5c59a865ebb48051e4d9d8ff7184c8dc5`, not moving main. If source changes, use a task-owned checkout of that SHA and preserve unrelated work. Never combine new source and old timing.
- Prefer the matching hosted log. If CLI retrieval fails, try `gh api repos/xlyk/ovrcr/actions/jobs/128576086691/logs`. If unavailable, retain missing measurements as unknown rather than substituting older-revision evidence, and use listing to recover identities as below. Continue all reachable source reviews; make unavailable checks explicit.
- If complete hosted output/source cannot reconcile selection, create a disposable macOS checkout and separate task-owned temporary directories for `TOOLS`, `CARGO_HOME`, and `CARGO_TARGET_DIR`. Set the private `CARGO_HOME` and `CARGO_TARGET_DIR` for both installation and listing. Install with `cargo install cargo-nextest --locked --version 0.9.151 --root "$TOOLS"`; do not alter global Cargo cache/configuration or PATH. Use the already-installed Rust toolchain; do not run a global rustup update. Verify nextest help, then invoke `"$TOOLS/bin/cargo-nextest" nextest list --workspace --all-targets --all-features --profile ci --message-format json -E 'not test(partial_resize_connection_delivers_error_before_owner_close)'`. Read binary IDs, ignored flags, and filter matches. This compiles/lists, not test-body execution; it supplies identities, not timings. Use isolated HOME/provider/XDG paths for any fixture execution; never touch live instances.
- If hosted execution evidence cannot be recovered, source-confirmed selected cases still receive full reviews; execution/timing claims without evidence remain unverified. Do not claim hosted runtime/count checks passed. A fresh full run or GitHub dispatch is not required by this review; no CI dispatch without separate authorization.
- Python logical-check totals are distinct from runner counts. Hosted guards are 40 per mode, correcting a static estimate of 41. Unknown per-check timing stays blank.
- `investigate` is a grounded completed finding with a concrete next verification, never unfinished reading. No promised pruning quota or runtime target.
- Nextest references: https://nexte.st/docs/machine-readable/list/ for JSON identity; https://nexte.st/docs/design/how-it-works/ for process-per-test behavior.


reviewer_model: openai-codex/gpt-6.1-sol

| lever | mechanism | expected effect (range) | risk | reversibility | verification |
|---|---|---|---|---|---|
| More nextest test threads | Let blocked PTY/socket/process tests overlap more work. Defaults follow logical CPUs; the measured 2.94 concurrent test processes are not CPU utilization. | [INFERENCE] Under the documented three-vCPU assumption, the current three-slot cap is nearly saturated, with little scheduler slack. A controlled trial that raises effective nonexclusive concurrency from about 3 to 4 has a **0–491 s execution-saving scenario**, assuming unchanged durations. Oversubscription can overlap blocked work; its benefit depends on the unmeasured blocked fraction and may be offset or reversed by contention. | CPU, executable-hash, disk, PTY and process contention can lengthen tests and trigger timing-sensitive failures. Four exclusive cases must remain exclusive. | One setting/CLI experiment; restore CPU default. | Compare controlled 3-slot and 6-slot runs on the same revision/runner class, including wall time, CPU busy time, top-binary duration deltas, retries and exact case coverage. See the experiment below. |
| Replace global exclusivity with one test group | `max-threads = 1` would serialize the four cases only against each other, while other cases run. | [INFERENCE] Recoverable slot-capacity penalty is only **0–24.827 s at three slots / 0–27.930 s at four slots**, excluding unknown drain/tail gaps. Their measured serial occupancy is 37.240 s, not 37.240 s of removable work. | **Does not preserve the current idle-runner protection.** Other tests can compete with the real 1500 ms authentication budget. Do not substitute a group without contention evidence. | Restore both `threads-required` overrides. | Check group membership and exact selected IDs; compare callback stage diagnostics and first-attempt outcomes under the rest of the suite. Preserve every assertion and the production deadline. |
| Move the four exclusive cases to a dedicated job | Run exactly those cases serially on a separate VM; exclude exactly those IDs from the main invocation. Keep the VM free of simultaneous build/Node/package work during their execution. | [INFERENCE] Same **0–25/28 s main-suite capacity gain** at 3/4 slots, plus possible drain-gap recovery. A standalone worker repays roughly **165 s setup + test compilation** at this run's costs, before 37.240 s execution. This is poor runner-minute value solely for four cases, though its duration can overlap the main job. | Lost IDs or a non-required job would remove notification/iTerm protection. CPU isolation is preserved only if the new worker does no other work concurrently. | Merge the filters back into the original job. | Require the job in `checks`; prove the two ID sets are disjoint and their union equals the original selected set. Compare case timings and retries. |
| Reduce global retries from 2 to 1 | Permit two total attempts instead of three. | **0 s observed saving:** both recovered tests passed on attempt 2. [INFERENCE] Saves one final attempt only for future tests that fail twice; there is no measured duration distribution for those attempts. | More hard-red shared-runner failures; changes the tolerance explicitly described by the config. Does not improve the defect itself. | Restore `retries = 2`. | Compare first-attempt failure records and eventual results over several runs; count third attempts before estimating benefit. |
| Retry only a named filter | Default to zero retries; grant the existing policy to an exact, owned list of timing-sensitive cases. | **0 s observed saving** if the two observed recovered cases remain listed. Removing their retry attempts would remove **26.115 process-seconds**, not 26.115 wall seconds, and this run would fail rather than provide equivalent green evidence. | New intermittent failures outside the list cause fail-fast and incomplete execution. Broad name patterns can silently admit new cases. CLI `--retries` overrides per-test policies. | Restore global retries. | Enumerate the effective overrides, retain FLAKY records, and compare identical selected IDs. Update the list from inspected failure evidence, not one run's apparent flake rate. |
| Quarantine list | Use a list for ownership/diagnosis while continuing to run every case and require its result. | **0 s automatic saving** if coverage and gating stay unchanged. Moving cases to a required concurrent lane can hide their time, with setup cost; making them nonblocking or skipping them is not equivalent coverage. | Nonblocking quarantine removes the merge gate for prompt privacy/lifecycle and suspended transfer effects in the two observed cases. That trade is not recommended. | Restore their normal required lane. | Require every lane in the aggregate and retain exact ID coverage and first-attempt results. |
| Change fail-fast | Existing `true` stops scheduling after an exhausted failure, not after a recovered failure. | **0 s attributable green-run delay or saving** in this run. [INFERENCE] Turning it off spends more time on failing runs but gathers more failures; no measured hard-failure run supports a number. | Disabling it abandons the stated “do not burn the rest of the job” policy. Across shards, local fail-fast does not automatically stop other jobs. | One boolean; restore `true`. | Use future genuine hard-failure logs to compare time after the final failed attempt and how much evidence would otherwise be lost. |
| Lower slow-timeout / terminate-after | Current policy reports slow at 60 s and begins termination at five periods, 300 s. | **0 s observed saving:** no attempt approached 300 s; max was 68.639 s. Three SLOW notices do not add waits. | A 60 s kill would destroy two real 60 s deadline tests. Timeout-as-pass would turn unfinished assertions into success. A shorter global deadline has no support from one run. | Restore `{ period = "60s", terminate-after = 5 }`. | Inspect timeout/termination records across runs and preserve legitimate deadlines. Measure runner shutdown/grace separately if a real hang occurs. |
| Two test jobs, binary-balanced | Split every nextest case by the explicit binary map below. Keep all features, target scope, the current exclusion and exclusive overrides. | [INFERENCE] At **three slots per worker**, ideal unchanged-duration execution is **1010.561 s**, versus 2014.918 s observed. Retaining the existing 400.082 s nonexecution envelope once gives **1410.643 s step-covered time**, a conditional **1004.357 s saving**. This is a model point, not an observed speedup or confidence interval; queue/build/contention changes can erase or reverse it. Four-slot sensitivity remains below. | Duplicate compilation/setup, runner queues, increased billed time, skew on another run, missing new binaries, incorrect aggregate gating. Filter-only splitting can still compile all test targets. | Collapse the matrix to the original invocation. | First prove exact union/disjointness for all 2820 main-run IDs. Then record each worker's queue, cache, compilation, execution, retries and total workflow critical path. |
| Three test jobs, binary-balanced | Isolate the indivisible 2380.841 s Pi binary; divide the rest as below. | [INFERENCE] At **three slots per worker**, ideal execution is **793.614 s**. Keeping the same 400.082 s nonexecution envelope gives **1193.696 s step-covered time**, a conditional **1221.304 s saving**. The third worker improves the two-worker ideal by **216.947 s**, before any extra critical-path costs. These are conditional model points, not measured forecasts; four-slot sensitivity remains below. | Same risks as two jobs, plus more runner minutes and queues. Pi alone exceeds one third of process work, so whole-binary splitting cannot be perfectly balanced. | Return to two jobs or one. | Same ID and timing checks. Compare the third job's extra runner cost with actual end-to-end improvement, not summed process seconds. |
| Tune existing rust-cache | Keep dependency caching; consider whether workspace artifact caching helps unchanged-revision reruns and shards. | Already present, with an exact ~784 MiB hit. **0 new saving from merely adding rust-cache again.** [INFERENCE] Additional warm-artifact reuse has a **0–136 s Test-build budget ceiling** in this run, before restore/save overhead; no measured hit rate supports a narrower range. | Larger caches, eviction, incompatible feature/compiler keys, and parallel writers. Workspace source changes invalidate reusable artifacts. | Remove extra cache inputs; retain existing action. | Record exact cache key, restored bytes/time, Cargo fresh/compiled units, link time and cold/warm results. Preserve compiler/features in keys. |
| Add sccache | Cache eligible compiler outputs across runs, beyond Cargo's on-disk freshness checks. | [INFERENCE] **0–136 s gross ceiling for the measured Test compilation phase**, realistically less because linker-invoking test/bin targets are not cacheable and dependencies already came from rust-cache. Net benefit can be negative after remote-cache/setup overhead. | More cache infrastructure with limited help for 56 linked test binaries. Clippy's check-only work is not the same as cacheable linked codegen. | Remove wrapper/backend and restore existing dependency cache. | Capture sccache hits, misses, noncacheable reasons and Cargo timings; compare net elapsed time with identical toolchain/features and cache conditions. |
| Narrow `--all-features` | Avoid GUI/diagnostic compilation in workers that do not need those configurations, while retaining those cases in a required feature-complete lane. | [INFERENCE] **0–136 s gross Test-build ceiling**, not a measured opportunity; per-feature compilation attribution is absent. No justified execution saving from dropping features wholesale. | Removing `gui` loses GUI-bin/test coverage; removing diagnostics bypasses conditional hash-count assertions. Feature unification can change test behavior even when test names match. | Restore identical all-feature commands everywhere. | Compare Cargo unit graphs/features and test-ID manifests, and keep the original feature-specific assertions exercised. Do not call a smaller manifest equivalent without examining its changed paths. |
| Align ignored-Pi build configurations | Investigate reuse of the two already-built targets without discarding their current non-GUI feature configuration. | Both **do recompile**: measured build phases **24.79 s + 8.34 s = 33.13 s**. [INFERENCE] **0–33.13 s gross ceiling** if reuse is genuinely equivalent. Simply changing them to all-features is not yet justified. | The main suite uses all features; these steps use only `acceptance-diagnostics`. Aligning flags changes the configuration tested. Excluding the installed host tests loses real SDK protection. | Restore the two exact Cargo commands. | Compare verbose Cargo fingerprints/features and executable hashes; demonstrate the same installed-host assertions under the intended feature configuration. Keep both exact ignored cases and nested Node cases. |
| Parallel doc tests | Run the default-feature doctest command in a separate required auxiliary job rather than behind nextest. | **0–49 s critical-path saving scenario**, before extra setup/queue costs. It ran zero doctests, but spent 44.92 s compiling and still checks the command/default-feature path. | Deleting the command loses the future doctest gate. Running Cargo concurrently in the same target directory can serialize on its lock and compete with timing-sensitive cases. | Return the step to its current position. | Retain `cargo test --workspace --doc`; compare default-feature build and five crate results, auxiliary duration and aggregate gating. |
| Parallel ordinary Node batch | Run source-based extension fixtures after Node 22 setup, independently of Rust execution. | **0–6 s critical-path saving scenario** including the measured 1 s Node setup and 5 s batch. Best as part of an existing auxiliary lane; its own worker's setup can outweigh its work. | Do not mix in installed-Pi child scripts; those need their Rust parents' materialized production argv. Same-VM concurrency can disturb exclusive cases. | Move setup/batch back. | Preserve the nine-file command and its 106 cases. Compare aggregate results, not only exit status. |
| Parallel packaging, Cursor and installed-Pi lane | Build a stable default-feature CLI, then package/check both profiles and run Cursor diagnostics. Provision Node/Pi before the two exact installed cases. | Packaging + Cursor have an **0–85 s overlap ceiling** from the 82 s + 3 s steps; the entire existing post-Test suffix has a **0–200 s ceiling**, including doctests, Node, Pi provisioning/tests, upload and cleanup. These ceilings overlap and must not be added. Fresh-lane build/setup can reduce or erase the gain. | Packaging hashes the callback executable; concurrent Cargo writes can invalidate the artifact being checked. Cursor requires that executable. Fresh workers do not inherit current default-feature artifacts or npm installation. | Restore sequential steps. | Build before packaging, keep the callback immutable through checks, preserve both package profiles and pinned hosts, then require the lane and its evidence upload in the aggregate. |

## Evidence and boundaries

This is an analysis of revision `992a1da5c59a865ebb48051e4d9d8ff7184c8dc5`, using [hosted run 42429838269, job 128576086691](https://github.com/xlyk/ovrcr/actions/runs/42429838269/job/128576086691). No product/config/workflow files were changed, and no builds or tests were run for this report.

Numbers marked **measured** come from that one run. Numbers marked **[INFERENCE]** are arithmetic scenarios, not confidence intervals or promised improvements. The range endpoints reflect stated assumptions; a changed run can fall outside them. In particular, additional contention, cold caches or queued runners can make an optimization slower. No empirical slowdown bound or population flake rate is available.

The API job interval is **2418 s** (`evidence/run.json:1`). The enumerated step-covered interval is **2415 s** (`evidence/job-steps.csv:2-24`). `evidence/cost-analysis.json:16-35` calls 2415 s “job wall”; its derivation is actually the step interval. This report keeps those definitions separate.

The exact workflow command includes `-E 'not test(partial_resize_connection_delivers_error_before_owner_close)'` (`.github/workflows/ci.yml:47-50`). Every proposed partition must preserve that existing exclusion, the ordinary ignored-test policy and the two later explicit ignored invocations. The workspace has five members (`Cargo.toml:1-4`). This report does not include the separate offline Cursor or manual Linux job (`.github/workflows/ci.yml:104-115,139-145`).

## Concurrency is occupied slots, not CPU busy time

The ledger sums to **W = 5931.968 process-seconds** (`evidence/attempts.csv:2-2823`; the independent summary is `evidence/cost-analysis.json:19-33`). The nextest summary reports **T = 2014.918 s** (`evidence/macos-job.log:3396-3398`). Thus **W/T = 2.944 occupied test slots on average**. A sleeping test still occupies a slot; subprocess CPU use is not reported by this number.

Nextest's [configuration reference](https://nexte.st/docs/configuration/reference/#profiletest-threads) sets the default to logical CPUs. The workflow/config does not override `test-threads` (`.github/workflows/ci.yml:47-48`; `.config/nextest.toml:7-22`).

### Documented three-vCPU assumption

The hosted log identifies **`Image: macos-26-arm64`** (`evidence/macos-job.log:17-18`; its image-documentation link is at line 19). [GitHub's standard-runner specifications](https://docs.github.com/en/actions/reference/runners/github-hosted-runners#supported-runners-and-hardware-resources) list arm64 `macos-latest`/`macos-26` as **3 CPUs (M1)** in both public and private repository tables. **[INFERENCE] Assume three vCPUs and the corresponding three-slot nextest default for this run.** This is an image/specification-based assumption, not a core-count measurement captured by the job; record `sysctl -n hw.logicalcpu` in a future controlled run. Four slots remain a sensitivity comparison, not a claim about this VM.

At a fixed three-slot cap, the elapsed-work lower bound is **W/3 = 1977.323 s**. Observed **T = 2014.918 s** is **101.9% of that bound**, or **98.1% occupied-slot efficiency**. The gap is **37.595 s**, including the unavoidable capacity penalty of global exclusivity and other scheduling effects. At a fixed four-slot cap, **W/4 = 1482.992 s**, which makes observed occupied-slot efficiency **73.6%**. Both calculations use the same ledger/summary cited above.

This establishes a nearly saturated **three-slot scheduler**, not saturated CPUs. W includes sleep, PTY reads, sockets and other elapsed waits. Raising `test-threads` above three changes the slot cap and can beat W/3 by overlapping blocked work. Any gain depends on the unmeasured blocked fraction of W, offset by CPU/PTY/disk contention and the exclusive intervals; it cannot be inferred from 98.1% slot efficiency alone. Separate VMs add both slots and CPU/resource capacity, so the two-job split has a stronger basis than assuming same-VM oversubscription provides equivalent capacity.

### Exclusive-aware slot model

The four exclusive tests occupy **E = 37.240 s**. A simple fully occupied, unchanged-duration model is:

`execution(c) = E + (W - E) / c`.

| Assumed available test slots / cores | Observed average slots divided by cores | Modeled execution with exclusivity | Observed time above model |
|---|---:|---:|---:|
| 3 | 98.1% | 2002.149 s | 12.769 s |
| 4 | 73.6% | 1510.922 s | 503.996 s |

These percentages are **slot utilization proxies**, not measured CPU utilization. Outside the exclusive intervals, `(W-E)/(T-E) = 2.981` average processes. On a three-core/three-slot runner, this is consistent with an almost continuously full scheduler. On a four-core/four-slot runner, the missing overlap needs explanation before prescribing more threads. Scheduler tails, process startup, interference, eligibility constraints and duration inflation are possible causes; this log alone does not separate them.

If nonexclusive work remains the same and effective concurrency rises from three to four, the model falls by **491.227 s**. Raising it to six would give 1019.695 s execution, but that is only an optimistic I/O-overlap calculation, not a recommended target or a CPU-capacity claim. CPU-bound work cannot simply acquire six cores on a three/four-core VM. Each test may also spawn children, making test-thread count an imperfect resource limit.

Recommendation: benchmark a modest increase, rather than applying `2 × CPUs` as a rule. Preserve global exclusivity while testing this. There is little scheduler slack to recover at the current three-slot cap; the trial tests whether overlapping blocked work outweighs added contention, not whether the existing scheduler can fill its slots.

### One controlled experiment

Run one matched pair at the pinned revision: the exact existing workflow selection/profile once with **`test-threads = 3`** and once with **`test-threads = 6`**, on the same documented runner class and with matched toolchain/cache state. Record logical CPUs; separate compilation and runner queue time from nextest execution. Keep retries, timeouts and all four global-exclusive overrides unchanged. Do not overlap packaging or other new work with either invocation.

For each run, collect nextest wall time, total attempt time, first-attempt failures, selected IDs, peak memory/process pressure, and **aggregate CPU seconds for each test process tree including descendants**. Process-tree CPU accounting matters because a test can spend most of its parent time waiting while its children use CPU. Sample runnable/blocked process states or off-CPU waits where available; elapsed minus CPU time alone is not an exact blocked fraction for multithreaded trees. Compare first-attempt duration distributions and totals for `pi_delegation`, `server_lifecycle`, `cursor_cloud`, `automatic_titles` and `retained_sessions`, rather than relying only on a shorter final summary.

Acceptance rule: retain six slots as a candidate only if **all original IDs and assertions finish, no additional first-attempt failures or timeouts occur, and nextest wall time falls without systematic first-attempt duration inflation in the top three binaries or degradation of the exclusive callback cases**. As an explicit trial threshold, require at least **5% execution-time reduction**; that is a chosen practical margin, not a statistical noise bound derived from this single run. A faster pair with stable per-test durations supports useful wait overlap; slower per-test durations with CPU/run-queue or disk pressure support contention. The pair measures the net scheduling response, not an exact blocked fraction or a reliable population speedup. Repeat matched pairs before a permanent change, and use the process-state/CPU evidence to estimate which waits actually supplied the extra overlap.

## The four exclusive cases

The exact filters are `.config/nextest.toml:15-22`. Only three notification cases are included, not every `desktop_notifications_bridge_click_*` test.

| Exact test name within `ovrcr::server_lifecycle` | Measured seconds | Source |
|---|---:|---|
| `desktop_notifications_bridge_click_discards_transient_drafts_and_releases_capture` | 10.796 | `tests/server_lifecycle.rs:13167-13247` |
| `desktop_notifications_bridge_click_focuses_existing_split_without_duplicating_target` | 6.902 | `tests/server_lifecycle.rs:13125-13163` |
| `desktop_notifications_bridge_click_lands_in_browse_without_reviewing_or_answering` | 9.960 | `tests/server_lifecycle.rs:13046-13121` |
| `iterm_setup_real_palette_requires_separate_opt_in_and_uses_current_owned_dashboard` | 9.582 | `tests/server_lifecycle.rs:20377-20537` |

Their timing records are `evidence/attempts.csv:931-933,972`. All passed on their first observed attempt.

[`threads-required = "num-test-threads"`](https://nexte.st/docs/configuration/threads-required/) reserves the entire current nextest pool. It prevents overlap with *every* other test in that invocation, including after an increase in `test-threads`. The config explicitly describes hosted PTY/socket/process timing races and an otherwise idle runner for the fixed **1500 ms owner-hash budget** (`.config/nextest.toml:1-6,19-22`). That deadline is real production behavior, shared by `navigate` and `owner` (`src/cli/bridge.rs:11-25,39-55`). Hashing is part of the deadline (`src/cli/bridge.rs:99-125`). The test also asserts that an identical named executable is not hashed twice (`tests/server_lifecycle.rs:13200-13212`).

The behavioral assertions protect discarded drafts, no unintended answers/review/spawns, focus of an existing split, explicit iTerm opt-in and current-owner authentication. Removing contention insulation does not remove those assertions, but makes their ability to finish within the real production budget less reliable. Extending that budget to make an oversubscribed configuration pass would change the behavior under test and is outside this recommendation.

With c slots, the exclusive periods leave `(c-1)E` unused slot-seconds: **74.480 at c=3**, **111.720 at c=4**. Their ideal wall penalty relative to fully overlap-capable tests is `E(1-1/c)`: **24.827/27.930 s**. There may also be scheduler drain gaps before them; the completed-attempt ledger does not measure those precisely. A process-local mutex (`tests/server_lifecycle.rs:32-46`) cannot replace nextest scheduling because tests run in separate processes. A [test group](https://nexte.st/docs/configuration/test-groups/) protects only members of that group. Keep the existing policy unless actual contention measurements show the weaker group boundary suffices.

## Retries, fail-fast and timeouts

The comment intends to tolerate timing-sensitive hosted failures while leaving a FLAKY list to harden; it expressly says retries hide the red, not the defect (`.config/nextest.toml:1-12`). [Two retries means up to three total attempts](https://nexte.st/docs/features/retries/).

| Recovered case | First failed attempt | Successful retry | Total process time |
|---|---:|---:|---:|
| `ovrcr::codex_initial_prompt` / `codex_harness_default_and_explicit_prompt_use_the_shared_lifecycle_and_privacy_path` | 4.855 s | 4.372 s | 9.227 s |
| `ovrcr::pi_delegation` / `parent_recovery::parent_recovery_transfer_discovers_prepared_and_suspends_accepted_unknown_partial_effects` | 24.507 s | 21.743 s | 46.250 s |
| Total | 29.362 s | 26.115 s | 55.477 s |

Source: `evidence/attempts.csv:342,345,717,720`; final summaries `evidence/macos-job.log:3397-3398` repeat those retries and add no executions.

There are two different counterfactual costs:

- **Marginal retry-policy work:** 26.115 process-seconds for attempt 2. If that work could be omitted while holding other work fixed, its scheduling contribution is between zero and 26.115 wall seconds; a full-slot capacity estimate is **8.705 s at three slots / 6.529 s at four slots**. In reality, zero retries would make this observed run red and activate fail-fast, so a faster red run is not an equivalent green-run optimization.
- **Cost of the underlying first-attempt failures:** 29.362 process-seconds would disappear if both tests had passed in their observed successful-retry durations on their first attempt. That is an inferred counterfactual, worth about **9.787/7.341 s** of full-slot capacity at 3/4 slots. Hardening the failures may pay that back; changing retry count does not harden them.

`retries = 1` changes no execution in this log. An exact retry allowlist also changes no observed work if it includes both recovered tests. A maintained quarantine list can improve ownership without skipping tests. A nonblocking list would specifically stop requiring prompt privacy/no-third-launch assertions (`tests/codex_initial_prompt.rs:440-496`) and transfer preservation of prepared/Accepted/Unknown/Partial effects (`tests/pi_delegation/recovery.rs:801-911`). Keep these required.

`fail-fast = true` and timeout checks do not intentionally sleep before passing a case. No exhausted failure occurred. All 2820 completed selected cases passed eventually (`evidence/macos-job.log:3396`). Thus there was no fail-fast cancellation or timeout-termination cost to recover from this green run. Small scheduler bookkeeping overhead is not separately measured; do not mistake “no policy wait” for a nanosecond-level benchmark.

There were exactly three 60 s SLOW notices (`evidence/macos-job.log:783,787,888`): the two Claude probe cases at **62.912/62.920 s** and the Cursor case at **68.639 s** (`evidence/macos-job.log:788-789`; `evidence/cost-analysis.json:149-179`). No attempt reached **300 s**, and the ledger contains only PASS and the two FAIL attempt results. `terminate-after=5` was **never reached in this observed run**; history beyond this run is unknown. The [Unix timeout behavior](https://nexte.st/docs/features/slow-tests/#how-nextest-terminates-tests) can also add the default ten-second graceful-shutdown window after 300 s, and exhausted retries could repeat a hanging attempt. That is a failure-path bound, not a current green-run cost.

## Two- and three-job splits

These are two alternatives, not cumulative recommendations. They keep **all 56 binaries and 2820 selected tests**. Each binary total below is the sum of its attempt ledger seconds, including recovered retries (`evidence/attempts.csv:2-2823`; top totals cross-check `evidence/cost-analysis.json:37-138`). The complete map makes ownership explicit. New binaries should enter a catch-all lane so an outdated curated list cannot drop them.

| Plan/lane | Process-seconds | Binaries | Modeled execution, 3 slots | Modeled execution, 4 slots |
|---|---:|---:|---:|---:|
| Two jobs A | 2974.765 | 4 | 991.588 s | 743.691 s |
| Two jobs B, contains exclusive cases | 2957.203 | 52 | 1010.561 s | 767.231 s |
| Three jobs A | 2380.841 | 1 | 793.614 s | 595.210 s |
| Three jobs B, contains exclusive cases | 1775.293 | 3 | 616.591 s | 471.753 s |
| Three jobs C | 1775.834 | 52 | 591.945 s | 443.958 s |

The lane model is `E_lane + (W_lane-E_lane)/c`, with 37.240 s reserved only in the lane containing `server_lifecycle`. It assumes enough eligible work throughout, identical per-test durations and simultaneous runner availability. Individual long-case tails, subprocess startup and a different fixture mix can invalidate it.

The two-job assignment is balanced within **17.562 process-seconds**. In three jobs, Pi's **2380.841 s** already exceeds the ideal third of **1977.323 s**. No whole-binary split can remove that bottleneck. The remaining work is balanced within **0.541 s**. Splitting Pi by module/test group could improve that balance, but introduces more selection rules and duplicate compilation of the same binary. Start with the simpler two-job split; measure whether the third VM's roughly three extra minutes of modeled critical-path improvement merits its cost.

### Setup and build costs are paid per worker

Measured pre-Test time is **61 s**: runner setup 3, checkout 6, toolchain 1, nextest 1, rust-cache restore 18, format 3, lint 29, and zero-second/skipped shell/git entries (`evidence/job-steps.csv:2-10`). The Test step then builds for **136 s**, runs nextest for **2014.918 s** and has **3.082 s** residual bookkeeping (`evidence/job-steps.csv:11`; `evidence/cost-analysis.json:18-33`).

A new test-only worker still pays the observed **29 s setup + 136 s compilation = 165 s** if it restores the same dependency cache and compiles the same target set. It need not repeat format/lint, but one required lane must retain them. Two/three workers add about **165/330 aggregate runner-seconds** of this repeated work relative to one worker under that assumption. They do not necessarily add 165/330 seconds to wall time: independent setup/builds overlap. Conversely, cache transfer, cache misses, queues or a compile-before-fanout dependency can put extra time onto the critical path.

The table's step-covered scenarios retain the entire current **400.082 s outside nextest execution** once on the critical path, then replace only execution with the slowest modeled shard. This includes existing setup, lint, compilation and the full post-Test suffix. **[INFERENCE] Under the documented three-vCPU/three-slot assumption**, the model gives **1410.643 s** for two jobs and **1193.696 s** for three, versus the measured 2415 s step-covered interval: conditional reductions of **1004.357 s / 1221.304 s**. The third worker improves the two-worker ideal by **216.947 s**. The four-slot sensitivity gives **1167.313 s / 995.292 s**, but is not the primary assumption. These model points are conservative about leaving the existing suffix serial, but optimistic about unchanged durations and queue/cache behavior. They are not measured forecasts or guaranteed bounds. They exclude the API interval's extra three seconds, any new aggregate-job overhead and the separate offline Cursor critical path.

The split is plausibly net positive for elapsed time because its modeled execution gain is much larger than warm-cache setup/build costs. It is not necessarily net positive for billed minutes. Measure both. Keep `checks` dependent on **every shard**, plus offline Cursor, and retain cancellation rejection (`.github/workflows/ci.yml:117-137`). A successful first shard must never stand in for the full suite.

An `-E` binary filter selects executions; it is not evidence that `--workspace --all-targets --all-features` compiles fewer targets. Budget the full 136 s until Cargo timings prove otherwise. A later build-once/distribute-artifacts design would need its own transfer/relocation and feature-identity measurements; it is not assumed here.

### Complete binary ownership

| Binary ID | Measured process-seconds | Two-job lane | Three-job lane |
|---|---:|---|---|
| `ovrcr` | 16.637 | B | C |
| `ovrcr-protocol` | 1.305 | B | C |
| `ovrcr-protocol::questions` | 0.133 | B | C |
| `ovrcr-runtime` | 154.868 | B | C |
| `ovrcr-runtime::delegation_cancellation_state` | 0.178 | B | C |
| `ovrcr-runtime::delegation_fallback_files` | 1.351 | B | C |
| `ovrcr-runtime::settings_matrix` | 1.789 | B | C |
| `ovrcr-runtime::theme_catalog` | 0.211 | B | C |
| `ovrcr-terminal` | 0.100 | B | C |
| `ovrcr-tui` | 32.767 | B | C |
| `ovrcr-tui::settings_theme_preview` | 14.270 | B | C |
| `ovrcr::agent_setup` | 6.443 | B | C |
| `ovrcr::automatic_titles` | 307.713 | A | B |
| `ovrcr::bin/ovrcr` | 4.747 | B | C |
| `ovrcr::bin/ovrcr-gui` | 15.298 | B | C |
| `ovrcr::claude_quota_probe` | 167.450 | B | C |
| `ovrcr::claude_reporting` | 8.874 | B | C |
| `ovrcr::claude_reporting_load` | 0.151 | B | C |
| `ovrcr::cli` | 66.545 | A | C |
| `ovrcr::client_startup` | 4.085 | B | C |
| `ovrcr::client_startup_server` | 61.644 | B | C |
| `ovrcr::codex_initial_prompt` | 13.765 | B | C |
| `ovrcr::codex_reporting` | 0.155 | B | C |
| `ovrcr::cursor_cloud` | 702.799 | B | B |
| `ovrcr::git_lifecycle` | 34.430 | B | C |
| `ovrcr::gui` | 99.716 | B | C |
| `ovrcr::initial_prompt` | 40.138 | B | C |
| `ovrcr::operator_auth` | 19.656 | B | C |
| `ovrcr::operator_e2e` | 53.059 | B | C |
| `ovrcr::operator_scope` | 1.962 | B | C |
| `ovrcr::operator_worker` | 0.191 | B | C |
| `ovrcr::pi_delegation` | 2380.841 | A | A |
| `ovrcr::pi_package` | 0.203 | B | C |
| `ovrcr::pi_reporting` | 0.873 | B | C |
| `ovrcr::provider_quota` | 134.734 | B | C |
| `ovrcr::quota_schema` | 0.162 | B | C |
| `ovrcr::quota_ui_option_a_smoke` | 2.520 | B | C |
| `ovrcr::registry_migration` | 16.677 | B | C |
| `ovrcr::remote_web` | 9.331 | B | C |
| `ovrcr::resource_cli` | 34.610 | B | C |
| `ovrcr::retained_capacity` | 57.052 | B | C |
| `ovrcr::retained_ownership` | 94.812 | B | C |
| `ovrcr::retained_sessions` | 219.666 | A | C |
| `ovrcr::server_lifecycle` | 764.781 | B | B |
| `ovrcr::service` | 1.165 | B | C |
| `ovrcr::settings_lifecycle` | 24.971 | B | C |
| `ovrcr::task_cli` | 2.491 | B | C |
| `ovrcr::task_execution` | 87.274 | B | C |
| `ovrcr::task_paging` | 5.423 | B | C |
| `ovrcr::task_runner` | 7.080 | B | C |
| `ovrcr::task_schedule` | 0.196 | B | C |
| `ovrcr::task_tui` | 0.992 | B | C |
| `ovrcr::terminal_acceptance` | 154.485 | B | C |
| `ovrcr::tui` | 36.527 | B | C |
| `ovrcr::wip_save` | 10.825 | B | C |
| `ovrcr::workspace_identity` | 51.847 | B | C |

## Compilation and cache limits

The workflow comment says Clippy covers compilation so it avoids a preceding `cargo check` (`.github/workflows/ci.yml:42-48`). It does **not** say that Clippy produces all linked test executables. Measured Clippy finished its `dev` checking phase in **28.00 s**, while nextest subsequently compiled workspace crates into its `test` targets in **136 s** (`evidence/macos-job.log:480-505`). The whole Lint step is 29 s. Retain Clippy's diagnostics and the actual test-binary build; adding `cargo check` would duplicate checking, not remove the build.

The existing cache is enabled with `shared-key: checks` (`.github/workflows/ci.yml:29-32`). The run restored an exact **822410196-byte / ~784 MiB** cache in the measured 18 s cache step (`evidence/macos-job.log:435-450`; `evidence/job-steps.csv:6`). It set `cache-workspace-crates: false` and `CARGO_INCREMENTAL=0` (`evidence/macos-job.log:389-400`). Thus “install rust-cache” is already done, and “there was a cache miss” is false for this run.

The [rust-cache action documentation](https://raw.githubusercontent.com/Swatinem/rust-cache/v2/README.md) describes dependency artifacts as its default cache, with workspace crates excluded and incremental artifacts removed. It keys on the toolchain and build environment. Reusing the shared dependency key across shards can avoid dependency downloads/codegen, but does not give new jobs the workspace binaries produced by another simultaneously running worker. Multiple workers can also restore the same old cache before any worker saves new artifacts.

[sccache's Rust limitations](https://raw.githubusercontent.com/mozilla/sccache/main/docs/Rust.md) exclude invocations that require the system linker, including bins, and require supported output modes including `link`. That constrains both the benefit for test executables and reuse of Clippy's checking work. Existing `CARGO_INCREMENTAL=0` is compatible with sccache, but this fact alone establishes no time saving. Ask for Cargo timing and sccache noncacheable-reason data before introducing a second caching layer.

Root features are `gui` and `acceptance-diagnostics`; the GUI binary requires `gui` (`Cargo.toml:60-67`). Nextest and Clippy select all features, but doctests and the packaging CLI use default features, and installed Pi uses only `acceptance-diagnostics` (`.github/workflows/ci.yml:44-54,81-94`). The real extra compilations are:

| Phase | Measured build time | Evidence |
|---|---:|---|
| Main nextest test binaries | 136 s | `evidence/macos-job.log:496-502` |
| Default-feature doctest preparation | 44.92 s | `evidence/macos-job.log:3408-3413` |
| Default-feature packaging CLI | 8.31 s | `evidence/macos-job.log:3463-3464` |
| Installed Pi package target | 24.79 s | `evidence/macos-job.log:4373-4376` |
| Installed Pi child-tools target | 8.34 s | `evidence/macos-job.log:4441-4443` |

These **222.36 s** are build-phase elapsed times in a serial workflow, not isolated compiler CPU time. They include different profiles/targets/features, and are not all redundant. The feature-set explanation for Pi rebuilding is **[INFERENCE]**, consistent with the distinct commands and fresh compiler lines; the log lacks unit fingerprints to prove the complete reason. Changing ignored-Pi steps to all-features might enable reuse but would remove their current non-GUI execution configuration. Keep that configuration unless its equivalence is demonstrated or preserved in another required lane. There is no safe measured 33.13 s saving to claim today.

## Work outside nextest and dependency order

Within the **2415 s step-covered interval**, the **2014.918 s nextest execution** leaves **400.082 s outside nextest execution**. This consists of **139.082 s inside the Test step** (136 s build + 3.082 s residual), plus **261 s in all other steps** (61 before Test + 200 after). Source: `evidence/job-steps.csv:2-24` and `evidence/cost-analysis.json:18-33`. If “outside nextest” instead means outside the entire workflow Test step, the answer is **261 s**, not 400.082 s.

The after-Test work is small compared with nextest, but most has no semantic need to await completion of the tests:

| Work | Measured step duration | What must precede it | Safe parallel placement to evaluate |
|---|---:|---|---|
| Default-feature doctests | 49 s | Checkout, Rust/dependency setup; builds its own default-feature libraries | Required auxiliary worker, independent of nextest completion. Keep the default-feature command even though all five suites currently report zero cases (`evidence/job-steps.csv:12`). |
| Startup packaging/checks, production and local-development | 82 s | Build default-feature `target/debug/ovrcr`; each package must exist before its checks | Auxiliary worker after its CLI build. Retain both modes. Packaging fingerprints/reads the executable and builds the bridge (`scripts/package-startup.sh:26-56,65-91`), so do not rebuild that path while checking it. |
| Relocated Cursor checks | 3 s | Pinned bridge archive installation and stable compiled CLI (`.github/workflows/ci.yml:64-69`) | Same auxiliary worker after CLI build; no need to wait for Rust test execution. |
| Ordinary extension batch | Node setup 1 s + batch 5 s | Checkout/source and Node 22 (`.github/workflows/ci.yml:70-74`) | Auxiliary worker before/alongside its other nonconflicting work. Source-based fixtures substitute helpers (`tests/omp_reporting_extension.mjs:16-29`); they do not depend on completion of nextest. |
| Provision installed Pi | 7 s | Node/npm and network access, pinned `@earendil-works/pi-coding-agent@0.87.1` | Can start independently of nextest in the auxiliary worker. Keep `--ignore-scripts` and evidence copies (`.github/workflows/ci.yml:75-80`). |
| Installed Pi parent tests | 31 s + 16 s total steps; test-body reports 5.37 s + 7.25 s | Installed host path, Rust target builds, production materialization/assignment fixture | Required auxiliary worker after provisioning/build. Nested Node cases remain within their parents, not in the ordinary batch (`.github/workflows/ci.yml:81-94`; `evidence/job-steps.csv:18-19`). |
| Evidence upload | 1 s | Evidence-producing installed-Pi steps; run even on failure | Keep `if: always()` and include shard identity if splitting uploads (`.github/workflows/ci.yml:95-102`). |
| Post-step cleanup | 5 s | Completion of dependent work | Do not count this as removable testing work (`evidence/job-steps.csv:21-24`). |

The listed durations come from `evidence/job-steps.csv:12-24`. Independent **jobs** offer resource isolation. Backgrounding everything on the same VM does not: it can take CPU/disk from the four “otherwise idle runner” cases, and simultaneous Cargo commands can contend for shared build locks. Build/package ordering must preserve a stable callback hash. Default-feature library work is currently warmed by doctests before the 8.31 s packaging CLI build; a fresh packaging worker cannot assume that the CLI alone still costs 8.31 s.

The entire measured suffix is 200 s. That is the maximum existing suffix that could be hidden behind nextest under unchanged costs, not an additional 49 + 85 + 6 + 200 s. Neither this overlap ceiling nor binary-shard scenarios should be added to thread-count savings without a new end-to-end scheduling model.

## Ordered by expected value per unit of risk

1. **Evaluate two required, binary-balanced Rust test workers.** The largest plausible elapsed-time gain preserves assertions, feature scope and global isolation within each VM. Exact selection/gating checks are essential. Measure warm/cold cache and queue costs before choosing it permanently.
2. **Move the existing post-Test checks into one required auxiliary lane.** Keep the default-feature build, pinned dependencies and artifact ordering intact. This has a much smaller, at-most-200 s measured overlap opportunity, but preserves behavior without changing deadlines or retry tolerance.
3. **Trial a modest thread-count increase with CPU and first-attempt measurements.** Easy to reverse; potentially useful for I/O waits, but it directly increases contention and cannot be justified from 2.94 slot occupancy alone.
4. **Assess a third test worker after measuring two.** Under three slots per worker, its ideal gain is another **216.947 s** with these whole binaries (four-slot sensitivity: 172.021 s). Reject it if queue/build/billing costs dominate.
5. **Inspect build-unit/cache data before adding workspace caching or sccache.** There is a measured 136 s main-build budget, but dependency caching already hit and linked test targets limit sccache's reach.

## Not worth it on current evidence

- Reducing retries from two to one **as a speed fix**: it saves zero observed work. Retrying only the two named cases also saves zero observed work and increases maintenance/coverage-completion risk.
- Relaxing global exclusivity to a group solely to reclaim roughly 25–28 s of modeled capacity. It removes protection for a real fixed authentication deadline. A standalone job solely for those four cases repeats far more setup/build time than their execution.
- Changing fail-fast or the five-minute timeout to improve this green run. Neither imposed a recoverable policy wait; lowering the timeout to 60 s would interrupt real deadline tests.
- A separate worker solely for the five-second Node batch. Put it in an existing auxiliary lane instead.
- Deleting the currently zero-case doctest command, dropping GUI/diagnostic features, making quarantine nonblocking, changing timeout to success, or skipping installed-Pi/package modes. Each trades away a current or future gate rather than making the same coverage faster.
- Adding `cargo check` before Clippy, adding rust-cache a second time, or assuming Clippy eliminates the need to link test executables.

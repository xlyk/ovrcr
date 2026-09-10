# Task 8 hosted Linux memory fixture

Implementation checkpoint, 2026-09-10. **Behavioral acceptance is unrun.** The fixture is ignored by ordinary test runs and must not run on the busy local macOS host. Source baseline at final compile/lint check: `c1699390e12579f826a100488720a40daa06422a`, plus the uncommitted test/workflow changes described here. The coordinator will record the eventual committed revision and hosted result.

## Feasibility and selected scope

The existing `linux-capacity` job uses `ubuntu-latest`; `gh repo view --json visibility` confirmed the repository is PRIVATE. Current [GitHub runner documentation](https://docs.github.com/en/actions/reference/runners/github-hosted-runners) lists private-repository standard Linux runners as 2 CPU, 8 GB RAM and 14 GB SSD. The nominal specification is not a substitute for the fixture's live preflight.

The new separate `linux-memory` job compiles the existing load test binary, records revision/OS/toolchain/memory/disk/cgroup provenance, runs one exact opt-in test with a 15-minute step timeout, and always uploads its evidence. It does not replace or alter `fifty_session_reporting_capacity` or that test's 3 GiB sampled-growth acceptance.

Exact new test:

```sh
OVRCR_MEMORY_EVIDENCE=<isolated-evidence-directory> cargo test --test claude_reporting_load --features acceptance-diagnostics fifty_collector_raw_index_and_sequential_json_high_water -- --ignored --exact --nocapture
```

## Real helper path and synchronization

The test reuses `CollectorController`, the real `ovrcr __agent-collector` executable, production JSON parsing/index accounting, the existing process-tree RSS sampler, and the controller's owned process-group cancellation. It uses one shared source file and 50 independently owned collector processes. It does not start Claude, a dashboard, or 50 native providers.

1. Start 50 helpers against an empty source; obtain an EOF acknowledgement from every helper and measure the empty-helper baseline.
2. Append 65,536 valid assistant records with unique 72-byte request and message IDs. Each entry charges 112 fixed bytes plus 144 ID bytes, reaching exactly 16 MiB while also reaching the identity count ceiling. Require every helper's acknowledged snapshot to show 65,536 identities, 16 MiB charged bytes, 393,216 canonical input tokens and no diagnostic. Nonzero output, cache and reasoning totals are also asserted: 327,680 output, 131,072 cache-read, 65,536 cache-write and 262,144 reasoning tokens.
3. Append one exactly 32 MiB JSON record without its terminating newline. It contains 96 nested array levels, a 65,536-field object and a padding string. Stop advancing each helper after its EOF acknowledgement. All 50 are held at the raw-record boundary while retaining their full indexes. No sleeps establish this barrier; readiness comes from actual collector responses.
4. Append the newline. Advance only one helper at a time so JSON parsing peaks are sequential. The record has a new identity: the expected result is `accounting_limit`, unchanged 65,536 retained identities/16 MiB charged bytes, and full `UsageTotals` equality with that helper's pre-parse snapshot, including scope, coverage, input/output, cache-read/cache-write and reasoning output. This proves the previously incomplete record reached valid JSON parsing and index admission. Silently dropping it would not produce the expected diagnostic; malformed JSON would produce a different diagnostic.
5. Record memory after each phase and during bounded scan/parse polling. Cancel all 50 owned groups, require ESRCH for every recorded group, remove the task-owned source directory and assert its absence. Existing controller destruction also owns helper cancellation if an assertion panics.

Preflight fails the test clearly unless `/proc/meminfo` reports at least 6 GiB currently available RAM and the source directory's filesystem reports at least 512 MiB free disk. A live check stops the fixture if available RAM falls below 1 GiB. This is a preflight for the selected fixture on an isolated standard hosted VM, not a general cgroup-aware admission controller. Host cgroup provenance is retained for review. One ten-minute work deadline covers helper startup, source construction, phase barriers and sequential parsing. Each 180-second phase barrier and 15-second parse deadline is capped by that absolute deadline. Exceeding it fails the test and unwinds owned helper guards. Normal cleanup permits 50 existing two-second cancellation budgets (100 seconds total), leaving more than three minutes before the 15-minute CI step backstop. Unexpected OS/filesystem stalls remain subject to that outer backstop; it is not an assertion of real-time scheduling guarantees. These precautions do not turn sampled availability into a proof that no transient spike occurred.

## Evidence and limitations

`memory.csv` keeps aggregate sampled process-tree RSS in separate rows from each process's `VmRSS` and `VmHWM`. Parent/test process memory is recorded separately alongside collector PIDs. A sum of individual high-water marks must not be described as a simultaneous aggregate peak.

The [Linux kernel proc documentation](https://docs.kernel.org/filesystems/proc.html) defines `VmHWM` as peak resident set size and cautions that RSS-related accounting is asynchronous and may be imprecise. Accordingly this is **kernel-reported process RSS high-water**, not precise allocator peak allocation, a universal heap bound, or an exact instantaneous aggregate maximum. Faster phase sampling does not change that distinction.

This fixture establishes only simultaneous raw-record/index retention and sequential parsing observations for its particular valid shape. It does **not** exercise 50 simultaneous pathological JSON expansions, every valid JSON shape, full dispatcher/dashboard queues, native provider process memory, or maximum connection counts. Its many-small-field object occupies part of a padded record; it is not a maximal-expansion construction. It does not resolve BTree node overhead, Vec capacity and allocator fragmentation, thread stacks, page cache, pipe/socket/PTY kernel buffers, or the unbounded server connection count identified in `inventory.md`. No actual socket-buffer measurements are produced by this collector-only fixture. Those Task 8 terms and macOS high-water remain open.

## Local verification

Only compile/lint/format checks ran; no high-water execution or claimed behavioral RED/GREEN result:

```text
rtk proxy env CARGO_INCREMENTAL=0 cargo check -p ovrcr --test claude_reporting_load --features acceptance-diagnostics
exit 0; completed in 2.32s

rtk proxy env CARGO_INCREMENTAL=0 cargo clippy -p ovrcr --test claude_reporting_load --features acceptance-diagnostics -- -D warnings
exit 0; completed in 1.24s

rtk proxy env CARGO_INCREMENTAL=0 rustfmt --edition 2024 tests/claude_reporting_load.rs
exit 0

rtk proxy git diff --check -- tests/claude_reporting_load.rs .github/workflows/ci.yml
exit 0
```

Disk was checked before compilation (942 MiB available before check, 926 MiB before final lint). The existing capacity test body remains unchanged. No staging, commit, push, workflow dispatch, native GUI action, or unrelated source edit was performed by this worker. The coordinator owns independent review, hosted execution, exact-revision evidence retention and work-diary closeout.

## Independent-review corrections

The first review of `87529055b56a5fc5093ee50ae0c059ff17c45aa5` found that the sum of per-phase bounds could outlast the CI step and that unchanged usage was asserted for only input/output. The correction above introduces a shared ten-minute work budget and full before/after `UsageTotals` equality. Per-record nonzero cache and reasoning values ensure those fields are meaningfully exercised.

Correction checks on `dd08be07be470e380945a938ce9066bcdd8c0c20` plus the uncommitted correction:

```text
rtk proxy env CARGO_INCREMENTAL=0 cargo check -p ovrcr --test claude_reporting_load --features acceptance-diagnostics
exit 0; 0.34s
rtk proxy env CARGO_INCREMENTAL=0 cargo clippy -p ovrcr --test claude_reporting_load --features acceptance-diagnostics -- -D warnings
exit 0; 0.34s
```

Rustfmt and scoped diff whitespace checks also passed. Disk was checked before compilation (792 MiB available). No local high-water execution occurred, and hosted behavioral acceptance remains pending. These corrections require independent re-review before hosted execution.

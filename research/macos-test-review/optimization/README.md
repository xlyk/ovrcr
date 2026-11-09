# Speeding up the `macOS checks` test runtime

Inputs: the per-test dataset (`../tests.csv`), the attempt ledger (`../evidence/attempts.csv`), the
review rounds, and four independent analyses in this directory (`accounting.md`,
`mechanisms.md`, `runner-and-ci.md`, `portfolio.md`). Every number below is measured from hosted run
[42429838269, job 128576086691](https://github.com/xlyk/ovrcr/actions/runs/42429838269/job/128576086691)
at revision `992a1da5c59a865ebb48051e4d9d8ff7184c8dc5`, or is labelled as a model estimate.

## Bottom line

The 30 minutes are **not** bound by test count, by redundant tests, or by thread count. They are bound
by **elapsed work inside a small number of heavyweight tests** — sessions, fixtures and deliberate
deadlines — and the cheapest large win is **buying more test slots by splitting the job**, not by making
the tests faster.

| Lever | Evidence it is the constraint | Estimated effect |
|---|---|---|
| Split the suite across 2 CI jobs | W = 5,931.968 process-s at 2.944 average occupancy on a 3-vCPU runner | **≈ −1,000 s** of step-covered time (idealized) |
| Cut per-session fixture cost | ~5.6 s per PTY session in `cursor_cloud`; PTY-annotated work is 34.5 % of wall | −45 … −70 s (that suite alone) |
| Shrink the two 60 s deadline budgets | 2 × ~62.9 s observed | −30 … −36 s, **only if** the assertion still proves the deadline |
| Prune / consolidate | all 28 groups together hold **4.344 process-seconds** | **−1.5 s (0.07 %)** — not a runtime lever |

## Where the time actually goes

Measured: cumulative test-process time **W = 5,931.968 s**; nextest execution **T = 2,014.918 s**;
step-covered interval **2,415 s**; job record **2,418 s**; compilation inside the Test step **136 s**.
`W/T = 2.944` average occupied slots — occupied *slots*, not CPU seconds: W sums elapsed durations,
including sleeps and blocked reads.

`macos-job.log:18` shows `Image: macos-26-arm64`, which GitHub documents as a **3-vCPU** runner. Treat
that as an assumption, not a measurement: nobody recorded `hw.logicalcpu`.

With k slots and the four globally exclusive tests (E = 37.240 s) held fixed, the work floor is
`E + (W−E)/k`:

| | k = 3 | k = 4 |
|---|---:|---:|
| Floor | 2,002.149 s | 1,510.922 s |
| Occupied-slot efficiency | 98.1 % | 73.6 % |
| Unexplained remainder | 12.769 s | 503.996 s |

At three slots the scheduler is nearly saturated: the wall time *is* the work. At four slots there is
503.996 s of unexplained slack — worth understanding, but no measurement in this run identifies it.

Work is diffuse, so there is no single hot spot to optimise:

| Band | Tests | Process-seconds | Share |
|---|---:|---:|---:|
| < 0.1 s | 2,043 | 49.1 | 0.8 % |
| 0.1–5 s | 701 | 1,341.1 | 22.6 % |
| ≥ 5 s | 427 | 4,563.9 | 77.0 % |
| ≥ 20 s | 37 | 1,238.3 | 20.9 % |

Top 10 tests = 8.7 % of W; top 100 = 37 %; top 300 = 65 %. The slowest single test is 68.639 s, 1.2 %.

First-match classification of `cost_drivers` across all nextest rows, with disjoint classes
(`accounting.md`):

| Mechanism class | Process-seconds | Share of W | Share of T at k = 3 |
|---|---:|---:|---:|
| PTY / pseudo-terminal | 2,082.765 | 35.1 % | 34.5 % |
| Polling | 1,533.663 | 25.9 % | 25.4 % |
| Other fixture / lifecycle / time | 818.559 | 13.8 % | 13.5 % |
| Storage / Git / filesystem | 670.752 | 11.3 % | 11.1 % |
| Process / spawn | 411.319 | 6.9 % | 6.8 % |
| Fixed wait / settle | 147.064 | 2.5 % | 2.4 % |
| Socket / network / thread | 125.327 | 2.1 % | 2.1 % |
| No positive mechanics annotation | 142.519 | 2.4 % | 2.4 % |

These are whole-test envelopes, not measured avoidable overhead: a test annotated "PTY" may spend most
of its time asserting behaviour *through* a real terminal, which is the coverage.

`mechanisms.md` also ranks mechanism *exposure* (process/PTY 4,422.670 s; polling 2,378.456 s;
SQLite/fsync 1,906.336 s; Git/network 1,010.919 s; fixed-sleep candidates 331.724 s; deadline budgets
324.486 s; exclusive scheduling 37.240 s). Those totals **overlap** — a single test can appear in four of
them — so they must never be added together or summed with the disjoint partition above. Use the
disjoint table for budgeting and the exposure table for discovery.

## Lever 1 — buy slots: split the suite into two jobs (biggest win)

The suite already runs at ~98 % of its three-slot floor, so no amount of test tuning inside one job
beats `W/3`. Two jobs each running at three slots halves the wall.

Binaries partition almost perfectly: longest-processing-time greedy over the 56 binaries gives
**2,966.0 s / 2,966.0 s** — exactly `W/2`, zero imbalance. At three slots that is ~989 s execution per
job instead of 2,014.918 s.

- Added cost per extra job: checkout + toolchain + the measured 136 s test build (≈ 300 s setup envelope),
  which runs concurrently with the other job's tests.
- Modeled step-covered critical path: ~301 s setup + ~989 s execution ≈ **1,290 s**, i.e. **≈ −1,100 s**
  against 2,415 s.
- Independent analysis in `runner-and-ci.md` puts the same lever at 1,167–1,411 s step-covered
  (≈ −1,004 … −1,248 s), and notes a three-job split adds only 172–217 s over two.

Caveats that decide whether this is worth it: it doubles billed runner minutes and queued-runner risk;
the exclusion filter, all features and the four exclusive overrides must be preserved in both jobs; a
per-binary split still compiles every test target unless the split is by `-E` filters over a shared
build; and a new binary added later must be routed deliberately or it silently runs in neither job.
Choose the partition from `evidence/attempts.csv` sums, not from intuition.

## Lever 2 — per-session fixture cost (the real "make tests faster" lever)

The heavyweight suites spend seconds of *setup* per session, then assert quickly. The clearest slope is
`ovrcr::cursor_cloud` (702.799 s over 47 tests, 14.95 s average, 11.8 % of all process time):

| Test | Sessions | Seconds | Per session |
|---|---:|---:|---:|
| `managed_cloud_refuses_incompatible_bridge_before_account_requests` | 13 | 68.639 | 5.28 |
| `managed_send_uncertainty_and_malformed_evidence_never_retry_or_become_ready` | 9 | 56.663 | 6.30 |
| `managed_send_crash_and_discard_boundaries_preserve_uncertainty…` | 8 | 46.006 | 5.75 |
| `managed_recovery_account_source_and_unknown_outcomes…` | 6 | 37.103 | 6.18 |
| `managed_live_cancel_known_run_requires_durable_intent…` | 4 | 24.25 | 6.06 |

Eleven `cursor_cloud` rows declare session counts: **51 sessions, 286.6 s, 5.62 s per session**. If
session startup fell to ~2 s, that slice drops to ~102 s — **≈ −185 process-seconds, ≈ −63 s of wall**
at the observed occupancy; at 1.5 s, ≈ −210 s (≈ −71 s wall). The mechanism is per-session bridge/PTY
setup and teardown, and the fix is structural: one warm bridge reused across the assertions of a test,
or a shared server with per-test session isolation, rather than a fresh fixture matrix per case.

Per-test fixes, with the race each wait protects, are enumerated in `mechanisms.md`. The most concrete
one: `managed_cloud_refuses_incompatible_bridge_before_account_requests` (`tests/cursor_cloud.rs:371-438`)
builds a whole `Fixture` — private Git repo, server, worktree, shell — for each of thirteen version
cases. Hoist the repository and server out of the loop and give each case a fresh session and bridge
(restarting the bridge, which reads its adjacent config on startup) plus a case-local log slice; keep
all thirteen inputs and the exact no-account-call assertions. That is a 0–63.36 s planning envelope, not
a forecast. Where per-case cleanup cannot be proved, share only an immutable Git seed. Never share
fixtures across nextest processes.

## Lever 3 — deliberate deadline budgets (load-bearing; do not shorten the tests)

Two `claude_quota_probe` tests exist to prove a 60 s deadline: 62.920 s and 62.912 s. Together they are
125.8 s (≈ 2 % of W, ≈ 43 s wall). `mechanisms.md` puts them on its **do-not-touch** list: one catches
silent expiry, the other catches a probe that completes before usage, and the existing fake-clock test
does not replace real process expiry. The elapsed wait *is* the assertion.

So the only honest lever is the **production** budget: if the real deadline moved to 15 s, the tests
would still prove enforcement and the suite would save roughly 95–105 process-seconds (≈ 32–36 s wall).
Change the product contract deliberately, or leave these alone — do not shorten the test around a
production deadline that still waits 60 s.

## What does not help

- **Pruning and consolidating.** All 28 confirmed groups sum to **4.344 process-seconds** removable
  (≈ 1.4–1.5 s wall); the biggest single group is the 28-execution shared fixture at 3.345 s. The
  54 consolidate rows are fast unit tests. Consolidation is worth doing for maintenance and CI noise,
  not for speed. Do not let a runtime goal justify deleting coverage.
- **Raising `test-threads`** without measuring. At three slots the scheduler is 98.1 % occupied, so the
  gain can only come from overlapping *blocked* time, and the blocked fraction is unmeasured. It can
  also lengthen timing-sensitive tests and disturb the four exclusive cases. Run the controlled trial
  below before adopting it.
- **Retry tuning.** The two recovered retries cost 26.115 process-seconds (≈ 8.7 s wall). `retries = 1`
  would not have saved anything on this run, because both tests passed on attempt 2.
- **Timeouts and fail-fast.** Nothing approached `terminate-after = 5` (max test 68.639 s); fail-fast
  cost nothing on this green run.
- **Caches.** `rust-cache` already hits; sccache's gross ceiling is the 136 s compile phase, and its net
  effect is unmeasured. The two installed-Pi steps do recompile (~33 s total) under a different feature
  set — aligning them is a correctness decision, not a free win.
- **Doc tests.** `cargo test --workspace --doc` runs zero tests but costs ~49 s of the critical path;
  moving it to an auxiliary required job is a ~49 s saving, and its own runner setup may eat that.

## Measurement plan (do this before believing any number)

Nothing here is a speedup until it is measured on this runner class with matched toolchain and caches.

1. Record the hardware you actually got: `sysctl -n hw.logicalcpu`, `hw.ncpu`, and nextest's effective
   slot count.
2. For each candidate change, run the **exact** current selection and profile once as baseline and once
   changed, same revision. Record nextest wall time, per-attempt start/end (nextest's JUnit or
   `--message-format json` stream), total attempt seconds, first-attempt failures, and peak
   process/memory pressure.
3. For the thread question specifically: `test-threads = 3` versus `6`, keeping retries, timeouts and
   the four exclusive overrides untouched. Accept six slots only if **all original IDs still run,
   no new first-attempt failures or timeouts appear, nextest wall falls, and the top binaries
   (`pi_delegation`, `server_lifecycle`, `cursor_cloud`, `automatic_titles`, `retained_sessions`) do not
   show systematic first-attempt duration inflation.** A 5 % execution reduction is a practical trial
   threshold, not a noise bound.
4. To attribute setup versus assertion, add phase timing inside the heavy fixtures (fixture creation,
   behavioural wait, assertion, cleanup) — the current evidence has whole-process durations and prose
   annotations only, so "setup dominates" is a hypothesis with good support, not a measurement.
5. For flakiness, collect **repeated** runs (≥ 10 on the same revision) before calling anything flaky;
   one run with two retries establishes neither a rate nor a cause.

## Risks that outrank the runtime goal

`mechanisms.md` maintains the authoritative **do-not-touch list**: the two 60 s quota deadlines, the
30 s notification-permission expiry window with its 1.2 s silence check, forced cancellation (SIGSTOP +
20 s deadline + 5 s TERM grace), the 50-session admission and 64-effect history tests, the OAuth
slow-down interval, real locked/durable SQLite storage, the negative "nothing happened" settle windows in
titles/settings, and the real owner-hash authentication with its scheduler reservation. Each note names
the source lines and says what a faster substitute would stop proving.

- The four `threads-required = "num-test-threads"` tests protect a real 1500 ms owner-hash budget
  (`src/cli/bridge.rs:11-25`, `:99-125`). Serialising them with other load can turn a timing assertion
  into a coin flip. Keep them exclusive unless you can show isolation is unnecessary.
- The 60 s deadline tests exist to catch an unbounded wait. Shortening them without changing the
  production deadline removes the only proof that the deadline is enforced.
- Anything that moves tests between jobs must keep the aggregate gate required, or a whole job's
  failures stop blocking merges.

## Sources

| Report | Contents |
|---|---|
| `accounting.md` | utilization model, concentration, disjoint cost classes, headroom envelopes |
| `mechanisms.md` | per-test mechanism, concrete fix, risk, and saving range for the most expensive tests |
| `runner-and-ci.md` | thread/exclusivity/retry/timeout/job-split/compile levers, each evaluated separately |
| `portfolio.md` | ranked consolidations by seconds and protection, retry economics, double-count rules, ceilings |

All four were produced by `openai-codex/gpt-6.1-sol` agents working read-only; none ran a build, a test
or a formatter, and none modified product, test or configuration files.

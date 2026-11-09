# Plan to reduce macOS test runtime

Status: **executed and merged** (`1bc72591`, PR #412, 2026-11-09). The measured outcome is below; the plan text after it is the pre-implementation reasoning, kept as written.

## Outcome

Sharding was adopted, and three of this plan's predictions needed correcting by measurement:

| Prediction | Result |
|---|---|
| Four shards ≈ one CI run of ~15 min | **14.3–18.9 min** wall across six green runs, slowest leg 696–1105 s |
| Queueing is the residual risk | Confirmed — one run had a leg wait 421 s for an arm64 runner, another had no queueing at all (2–4 s) |
| Three shards might beat four | **No.** Three shards removed all queueing but grew per-leg work more than that saved: 1,029 s against 991 s |
| Hash balance is acceptable | Partly — counts balance exactly, durations do not: 12.6–51.7 % spread, costing 35–156 s |

Two things were added that the plan did not anticipate. Each leg now uploads its `cargo nextest list --message-format json` output, and a Linux `test selection audit` job fails the build when a target the workspace declares is absent from any leg — the coverage gate this plan listed as required but which the first implementation skipped, costing an afternoon of chasing a "flake" that turned out to be `main` deleting a test file. And `taiki-e/install-action@nextest` needed a retry: four legs fetching simultaneously failed twice in one run and took 68 s instead of 1 s.

Also corrected: the "`W/3` is a floor" claim in §Step 1 does not hold once concurrency exceeds three slots, and "doubles billed runner minutes" was wrong for a public repository where standard runners are free.

Remaining work from this plan, not started: moving the 151 s non-test suffix to its own lane, and duration-aware balancing. Per-session fixture cost is untouched and is covered by `fixture-work-plan.md`.

Evidence: hosted run 42429838269 at revision `992a1da5c59a865ebb48051e4d9d8ff7184c8dc5`, the per-test dataset `../tests.csv`, and the four analyses in this directory.

## Recommendation

**Shard the nextest selection across four jobs.** The job goes from 40m18s to a modeled **~15 minutes**, and it costs nothing:

- `xlyk/ovrcr` is a **public repository**, and standard GitHub-hosted runners are free there. Billing docs: standard runners in public repositories are free and do not draw from included compute usage.
- The free-tier cap is **5 concurrent macOS jobs** — not 20. So four test shards plus the offline Cursor job is exactly the ceiling, and the aggregate job must move off macOS.

This supersedes the earlier two-job proposal. That one was gated on a billing objection that does not apply to this repository.

## Why the ceiling is real

Two facts bound what sharding can buy:

| Fact | Value | Consequence |
|---|---:|---|
| Cumulative test-process time | 5,931.968 s | The work to divide |
| Slots per runner | 3 | Runner image `macos-26-arm64` |
| Fixed cost outside nextest | 400.082 s | Paid on every job's critical path |
| of which setup + build | ~197 s | Duplicated per shard, but overlapped |
| of which post-test suffix | ~200 s | Currently serial after tests |
| Max concurrent macOS jobs | 5 | Caps useful shard count |

Per-shard execution is `W / N / 3`, so the ladder is:

| Shards | Execution per job | Critical path (suffix serial) | Critical path (suffix parallel) |
|---:|---:|---:|---:|
| 1 (today) | 1,977 s | 40m18s measured | — |
| 2 | 989 s | 23.1 min | 19.8 min |
| 3 | 659 s | 17.6 min | 14.3 min |
| 4 | 494 s | **14.9 min** | **11.5 min** |
| 5 | 395 s | 13.2 min | 9.9 min |

The `N=1` row is the model's only sanity check: it lands within 44 s of the 2,418 s observed job. That gap is the rough size of scheduler tails and exclusivity the formula ignores, and it is why the shard rows are estimates rather than predictions. At four shards the critical path stops being test execution and becomes the ~197 s setup-plus-build plus whatever suffix work stays serial. Further shards buy less than they cost in complexity.

## Step 1 — four shards, hash-partitioned

Files: `.github/workflows/ci.yml:18-137`, `docs/development/testing.md:57-82`. `.config/nextest.toml` is unchanged.

```bash
F='not test(partial_resize_connection_delivers_error_before_owner_close)'

# in each shard, with N substituted
cargo nextest run --workspace --all-targets --all-features \
  --profile ci -E "$F" --test-threads=3 --partition hash:K/4
```

Structure:

| Job | Runner | Contents |
|---|---|---|
| `macos_test` (matrix ×4) | `macos-latest` | Setup, rust-cache, nextest shard K of 4, upload evidence |
| `macos_checks` | `macos-latest` | Format, lint, doctests, packaging both profiles, Cursor diagnostics, Node batch, Pi provisioning, the two ignored Cargo parents, artifact upload |
| `offline_cursor` | `macos-latest` | Unchanged |
| `checks` | `ubuntu-latest` | Aggregate: requires every job above to be `success`, plus cancellation rejection |

Peak macOS concurrency is 4 shards + 1 suffix job + 1 offline Cursor. That is six against a cap of five, so either the suffix job waits for a shard slot or you drop to three shards. Measure queue delay before deciding; if shards queue, prefer 3 shards with a parallel suffix job (14.3 min) over 4 shards that serialize on the cap.

**Why hash and not binary filters:** hash assignment is by full binary ID and test name, so new tests and new binaries enter automatically, and `pi_delegation` — 40% of all process time in one binary — spreads across shards. A whole-binary filter cannot split it, which is why a 4-way binary split would leave one shard holding most of the work.

**The risk that decides success:** hash balance is by *test count*, not duration. If a disproportionate share of the heavy tests lands in one shard, the slowest shard sets the wall time. Measure per-shard execution on the first run. If the spread exceeds about 15%, replace hashing with a duration-balanced explicit partition computed from `evidence/attempts.csv` — one explicit set plus its `not (...)` complement, never dual allowlists.

**Gates:**

1. Capture `cargo nextest list ... --message-format json` for the unpartitioned selection and each shard. Compare `(binary ID, full test name)` pairs; require disjoint shards whose union equals the 2,820-case baseline.
2. Every shard uses identical nextest version, features, profile and `TEST_THREADS`. The install action floats, so pin or record the resolved version (the evidence run used 0.9.151).
3. The four `threads-required = "num-test-threads"` cases stay in whichever shard contains `server_lifecycle`, with no other work on that VM during their run.
4. `checks` requires all six results, keeps `always()`, and keeps the cancellation rejection. Branch protection stays on the stable `checks` name. Never `continue-on-error`.
5. Exercise failed, skipped and cancelled dependency outcomes during rollout. A green shard must never substitute for a missing one.
6. Record queue-to-assign time per shard. Concurrency is capped, so a fifth job can silently add minutes to the critical path.

## Step 2 — take the suffix off the critical path

The ~200 s after the tests — doctests, both packaging profiles, Node, Pi provisioning, upload — currently runs serially behind a 33-minute test step. Once tests are sharded, it becomes the floor.

- **Doctests: 49 s for zero tests.** `cargo test --workspace --doc` compiles and runs nothing. Keep the step (it is a real gate for future doctests) but move it into the suffix job, where it overlaps tests.
- **Packaging: ~82 s.** It builds the default-feature binary and checks both profiles. Independent of the test shards, so it can run concurrently.
- **Aggregate on `ubuntu-latest`.** It is shell-only and takes about a minute. Moving it frees a macOS concurrency slot and costs nothing on a public repo.

Modeled with the suffix parallel: **~11.5 min** at four shards.

## Step 3 — reduce the work itself

Only this step helps every future run, and it is the only one that shortens tests rather than reallocating them. Details are in `fixture-work-plan.md`; the short version is seeding the fixture repository instead of five git spawns, replacing spawn-per-poll waits, and making `Live` construction lazy. Expect seconds to low minutes, not more. It also reduces flake surface, which matters more once four jobs can each go red.

## Coverage and product limits

- No test deleted, no assertion weakened, no deadline shortened, no selection narrowed.
- Keep the existing exclusion, the ignored-test policy, two retries, fail-fast, the 5-minute timeout and all four exclusive cases.
- Every auxiliary step runs exactly once. Do not duplicate packaging, Node, Pi provisioning or doctests per shard.
- Nothing moves to a non-required lane. A shard that is not required is a coverage hole.
- Do not change production deadlines to speed up the two 60-second quota tests. That is a product decision.

## Not recommended: a bigger runner

Checked because it is the obvious "throw hardware at it" answer, and it is blocked:

| Option | Reality |
|---|---|
| `macos-latest-large` (12-core Intel) | Larger runners require a Team or Enterprise **organization**; `xlyk/ovrcr` is a personal repo. Also billed even on public repos, at $0.077/min. |
| `macos-latest-xlarge` (5-core arm64 M2) | Same plan requirement, $0.102/min, and only 5 cores against today's 3 — about 1.7× slots for real money. |
| The 12-core Intel machine | Would additionally change the architecture under test from arm64 to x64, which is a coverage change, not a speed change. |

Free public-repo minutes only apply to standard runners. Sharding spends those free minutes; a larger runner spends dollars instead.

## Corrections to the earlier analyses

Four claims in `README.md` and `runner-and-ci.md` do not survive review and should be fixed before anyone implements from them:

| Claim | Correction |
|---|---|
| "No tuning inside one job beats `W/3`" | False. `W/3` assumes three slots and fixed work; six slots may overlap blocked waits, and fixture work lowers `W` itself. |
| Splitting "doubles billed runner minutes" | False for this repo. Public-repo standard runners are free; sharding spends free capacity. |
| Exactly 2,966 / 2,966 s binary balance | Unsupported by the concrete map, which gives 2,974.765 / 2,957.203 s. |
| `--message-format json` for runtime timing | `json` is documented for `list`. Runtime JSON is experimental (`NEXTEST_EXPERIMENTAL_LIBTEST_JSON`) and its retry coverage must be verified first. |

Also state the hardware assumption plainly: three vCPUs comes from the `macos-26-arm64` image specification. `hw.logicalcpu` was never logged. Add it to the new shards so the next analysis is measured rather than inferred.

## Delivery order

1. Correct the four claims above.
2. Land the shard matrix at N=4 with a pinned nextest version; keep the suffix job where it is at first so only one variable changes.
3. Verify list-manifest equality, aggregate gating, and per-shard balance on real runs.
4. Move doctests, packaging and the aggregate job in a second change.
5. Then, and only then, tune `test-threads` per shard — with four shards the slot cap matters less, and the earlier 3-versus-6 comparison is only worth running if shard balance shows slack.

Verification boundary: this pass recomputed the ladder arithmetic from the frozen ledger and confirmed repository visibility, runner specifications, pricing and concurrency limits against GitHub's current documentation. It ran no nextest invocation and changed no workflow. Every timing improvement remains a model. *(Superseded by the Outcome section at the top, where those figures were measured.)*

# Node inventory notes

Run 42429838269, job 128576086691, revision `992a1da5c59a865ebb48051e4d9d8ff7184c8dc5`.

## The 106-case batch

`.github/workflows/ci.yml` step `Extension tests` runs nine files in one
`node --test` invocation. The log emits one TAP stream:

| fact | value | log line |
|---|---|---|
| TAP plan | `1..106` | 4338 |
| reported tests | 106 | 4339 |
| passed | 106 | - |
| failed | 0 | - |
| reported duration_ms | 4242.999792 | 4346 |
| sum of per-case duration_ms | 7689.968 | - |

Seconds are the TAP `duration_ms` at 3 decimals. 17 cases run under
0.5 ms and would round to `0.000`, which the canonical inventory schema
forbids as a timing cell; those rows keep the identical measurement at full
printed precision (e.g. `duration_ms 0.161417` -> `0.000161417`). No value is
rounded up and none is inferred.

The sum of the per-case durations exceeds the stream's own duration
because `node --test` runs the nine files concurrently, so case durations
overlap; the batch row timings must never be added up or compared against
the stream total.

`# suites 0`: the reporter emits flat names, so `test_name` is the
exact TAP name with no describe prefix joined by ` > `.

Rows: 106 (one per runner-reported case) + 4 nested rows = 110.

| file | cases |
|---|---|
| `tests/pi_package_bootstrap.mjs` | 9 |
| `tests/pi_reporting_extension.mjs` | 23 |
| `tests/omp_reporting_extension.mjs` | 27 |
| `tests/pi_delegation_extension.mjs` | 17 |
| `tests/pi_interrupt_extension.mjs` | 4 |
| `tests/pi_question_extension.mjs` | 3 |
| `tests/pi_continuation_records.mjs` | 10 |
| `tests/pi_observation_extension.mjs` | 11 |
| `tests/pi_workflow_tracker.mjs` | 2 |
| total | 106 |

The TAP stream reports each file as one contiguous block in
declaration order; the block order in the log is completion order,
not the order of the files on the command line.

## Loop-generated cases

These cases come from a table/loop, so `source_start..source_end` is
the loop's line range (the runner still reports one case per element):

| TAP # | test name | loop line | file | source template |
|---|---|---|---|---|
| 29 | `fork cannot inherit a copied pointer` | 33 | `tests/pi_continuation_records.mjs` | ``${reason} cannot inherit a copied pointer`` |
| 30 | `new cannot inherit a copied pointer` | 33 | `tests/pi_continuation_records.mjs` | ``${reason} cannot inherit a copied pointer`` |
| 41 | `original intent recovers a lost Prepare before persistence` | 204 | `tests/pi_delegation_extension.mjs` | ``original intent recovers a lost Prepare ${loss} persistence`` |
| 42 | `original intent recovers a lost Prepare after persistence` | 204 | `tests/pi_delegation_extension.mjs` | ``original intent recovers a lost Prepare ${loss} persistence`` |
| 48 | `DeclareDelivery persists an exact private intent before transport and reconciles a lost reply` | 546 | `tests/pi_delegation_extension.mjs` | ``${operation} persists an exact private intent before transport and reconciles a lost reply`` |
| 49 | `Archive persists an exact private intent before transport and reconciles a lost reply` | 546 | `tests/pi_delegation_extension.mjs` | ``${operation} persists an exact private intent before transport and reconciles a lost reply`` |
| 50 | `prepare-only registered start reserves without effects and recovers none Prepare loss` | 644 | `tests/pi_delegation_extension.mjs` | ``prepare-only registered start reserves without effects and recovers ${loss} Prepare loss`` |
| 51 | `prepare-only registered start reserves without effects and recovers before Prepare loss` | 644 | `tests/pi_delegation_extension.mjs` | ``prepare-only registered start reserves without effects and recovers ${loss} Prepare loss`` |
| 52 | `prepare-only registered start reserves without effects and recovers after Prepare loss` | 644 | `tests/pi_delegation_extension.mjs` | ``prepare-only registered start reserves without effects and recovers ${loss} Prepare loss`` |
| 72 | `malformed or incompatible owner metadata refuses (8 bytes)` | 55 | `tests/pi_package_bootstrap.mjs` | ``malformed or incompatible owner metadata refuses (${selection.length} bytes)`` |
| 73 | `malformed or incompatible owner metadata refuses (2 bytes)` | 55 | `tests/pi_package_bootstrap.mjs` | ``malformed or incompatible owner metadata refuses (${selection.length} bytes)`` |
| 74 | `malformed or incompatible owner metadata refuses (39 bytes)` | 55 | `tests/pi_package_bootstrap.mjs` | ``malformed or incompatible owner metadata refuses (${selection.length} bytes)`` |
| 75 | `malformed or incompatible owner metadata refuses (33 bytes)` | 55 | `tests/pi_package_bootstrap.mjs` | ``malformed or incompatible owner metadata refuses (${selection.length} bytes)`` |
| 76 | `malformed or incompatible owner metadata refuses (8193 bytes)` | 55 | `tests/pi_package_bootstrap.mjs` | ``malformed or incompatible owner metadata refuses (${selection.length} bytes)`` |

## Nested cases under Rust parents

Two `cargo test` steps run one Rust test each; the Rust test spawns
`node --test` on a dedicated .mjs file and the nested TAP stream appears
under `--nocapture`. They are not part of the 106.

- step `Installed Pi package through production materialization`
  - parent test name (macOS-job.log:4379): `production_materializer_loads_local_package_with_one_injected_owner`
  - parent_test_id: `cargo-test|Installed%20Pi%20package%20through%20production%20materialization|ovrcr%3A%3Alib|production_materializer_loads_local_package_with_one_injected_owner|production`
  - suite component is `ovrcr::lib` per the shared construction;
    the reporting cargo target is `tests/pi_package.rs`
  - nested TAP lines 4394-4415, 2 cases, duration_ms 1492.879958
  - source slots: 2 (`tests/pi_package_installed.mjs`)
  - cargo reported at macOS-job.log:4428: `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out; finished in 5.37s`

- step `Installed Pi effective child tools through production assignment argv`
  - parent test name (macOS-job.log:4446): `coding::production_assignment_argv_preserves_installed_sdk_child_publication_tools`
  - parent_test_id: `cargo-test|Installed%20Pi%20effective%20child%20tools%20through%20production%20assignment%20argv|ovrcr%3A%3Alib|coding%3A%3Aproduction_assignment_argv_preserves_installed_sdk_child_publication_tools|production`
  - suite component is `ovrcr::lib` per the shared construction;
    the reporting cargo target is `tests/pi_delegation.rs`
  - nested TAP lines 4453-4476, 2 cases, duration_ms 1904.338042
  - source slots: 2 (`tests/pi_child_tools_installed.mjs`)
  - cargo reported at macOS-job.log:4481: `test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 259 filtered out; finished in 7.25s`

Timing deviation from the assignment brief: the brief stated per-child
timing is unmeasured because the parent's duration covers it. The parent
steps do print the nested TAP stream, and it reports `duration_ms` per
child, so those measured values are recorded (seconds, 3 decimals) with
`timing_basis=hosted-single-run`, `attempt_count=1` (each parent ran once
with `--test-threads=1`) and `failed_attempt_count=0`. No duration was
inferred from the parent step's wall time.

## Excluded file

1. `tests/pi_publish_contract_installed.mjs` — a `node:test` file with its
   own cases. It is NOT invoked by this job: it is absent from the nine-file
   batch, absent from both nested steps, and no other file in the job
   references it. It stays outside the scored inventory.
2. `tests/fixtures/pi/pi_publish_contract.mjs` — a helper module, not a test
   file: it exports `assertPublishContract` and declares no cases. The job
   does execute it indirectly, because both
   `tests/pi_child_tools_installed.mjs` (in this job) and
   `tests/pi_publish_contract_installed.mjs` (not in this job) import it.
   Being a helper, it contributes no row of its own.

## Source facts

- `tests/pi_package_installed.mjs`: 2 source slots (2 after loop expansion)
- `tests/pi_child_tools_installed.mjs`: 2 source slots (2 after loop expansion)

Both nested files fail closed without `OVRCR_TEST_PI_PACKAGE_ROOT`; the
job provisions the pinned installed host in the preceding step, which is why
they are separated from the ordinary batch.

The only other Node-shaped output in this job is none; every other
check is Python, inventoried in `inventory_python.csv`.

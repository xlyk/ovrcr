# Operator activity checks — 2026-10-30

Product source: `ace3077b0c6743648b96b6986c3cac9ce50e7a07` (base `5efc90909e4998bd134414033f067f8d9253a1a3`). Precommit runs report that base SHA plus the intended working diff; native build records the committed product. All test processes use the private HOME/config/socket/temp runner in [check.py](check.py). No real provider credentials or paid inference.

Raw combined stdout/stderr is preserved as deterministic gzip beside command/exit/byte/SHA metadata. These are original attempts; failures are retained.

| Attempt | Exit | Actual result |
| --- | ---: | --- |
| [clippy](checks/clippy.json) | 0 | Finished `dev` profile [unoptimized + debuginfo] target(s) in 7.05s |
| [fmt](checks/fmt.json) | 0 | no output |
| [green-focused-01](checks/green-focused-01.json) | 101 | test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 334 filtered out; finished in 0.10s |
| [green-public-01](checks/green-public-01.json) | 101 | test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 329 filtered out; finished in 0.02s |
| [green-public-02](checks/green-public-02.json) | 101 | test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 329 filtered out; finished in 0.02s |
| [green-public-03](checks/green-public-03.json) | 101 | test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 329 filtered out; finished in 0.01s |
| [green-public-04](checks/green-public-04.json) | 0 | test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 329 filtered out; finished in 0.07s |
| [green-public-suite](checks/green-public-suite.json) | 0 | test result: ok. 330 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 9.79s |
| [green-tui-library](checks/green-tui-library.json) | 0 | test result: ok. 336 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 14.95s |
| [green-unit-01](checks/green-unit-01.json) | 0 | test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 334 filtered out; finished in 0.08s |
| [green-worker-01](checks/green-worker-01.json) | 0 | test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 15 filtered out; finished in 3.90s |
| [native-gui-cli-build](checks/native-gui-cli-build.json) | 0 | Finished `dev` profile [unoptimized + debuginfo] target(s) in 5.18s |
| [red-public](checks/red-public.json) | 101 | test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 329 filtered out; finished in 0.01s |
| [red-unit](checks/red-unit.json) | 101 | test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 334 filtered out; finished in 0.01s |
| [red-worker](checks/red-worker.json) | 101 | test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 15 filtered out; finished in 3.78s |
| [workspace-check](checks/workspace-check.json) | 0 | Finished `dev` profile [unoptimized + debuginfo] target(s) in 4.76s |

## Exact commands

`clippy`:

```sh
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

`fmt`:

```sh
cargo fmt --all -- --check
```

`green-focused-01`:

```sh
cargo test -p ovrcr-tui --lib operator_activity -- --test-threads=1
```

`green-public-01`:

```sh
cargo test -p ovrcr --test tui operator_activity_draw_tracks_real_responses_and_clock -- --test-threads=1
```

`green-public-02`:

```sh
cargo test -p ovrcr --test tui operator_activity_draw_tracks_real_responses_and_clock -- --test-threads=1
```

`green-public-03`:

```sh
cargo test -p ovrcr --test tui operator_activity_draw_tracks_real_responses_and_clock -- --test-threads=1
```

`green-public-04`:

```sh
cargo test -p ovrcr --test tui operator_activity_draw_tracks_real_responses_and_clock -- --test-threads=1
```

`green-public-suite`:

```sh
cargo test -p ovrcr --test tui -- --test-threads=1
```

`green-tui-library`:

```sh
cargo test -p ovrcr-tui --lib -- --test-threads=1
```

`green-unit-01`:

```sh
cargo test -p ovrcr-tui --lib operator_activity_state_and_cadence -- --test-threads=1
```

`green-worker-01`:

```sh
cargo test -p ovrcr --test operator_e2e --features acceptance-diagnostics compiled_worker::operator_activity_waits_for_real_model_and_tool -- --exact --test-threads=1
```

`native-gui-cli-build`:

```sh
cargo build -p ovrcr --bins --features gui,acceptance-diagnostics
```

`red-public`:

```sh
cargo test -p ovrcr --test tui operator_activity_draw_tracks_real_responses_and_clock -- --test-threads=1
```

`red-unit`:

```sh
cargo test -p ovrcr-tui --lib operator_activity_state_and_cadence -- --exact dashboard::operator::tests::operator_activity_state_and_cadence --test-threads=1
```

`red-worker`:

```sh
cargo test -p ovrcr --test operator_e2e --features acceptance-diagnostics compiled_worker::operator_activity_waits_for_real_model_and_tool -- --exact --test-threads=1
```

`workspace-check`:

```sh
cargo check --workspace --all-targets --all-features
```

## Preserved failures

The three `red-*` commands each execute one behavioral test and fail on the absent Sending label before product edits. `green-public-01` corrected the test's context-row offset (rows minus six); `green-public-02` corrected its literal substring assumption about wide-glyph padding; `green-public-03` corrected use of a discarded pending List request by responding to its retained original ID. `green-focused-01` corrected an assumption that the intentionally clipped Managing prefix remained complete beside a long tool name; the assertion now checks the retained session/run suffix. Product assertions were not weakened for the requested activity behavior. The full owning suites then pass.

Workspace full startup/acceptance suites and final-head hosted macOS/Linux/capacity/memory remain the controller's gates; they are not claimed from these local checks.

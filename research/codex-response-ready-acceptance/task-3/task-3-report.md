# Task 3 worker report

Implemented on `codex/codex-reporting`, baseline `26be5eb8ba2728135728192591283a2c906324de`, in `/Users/xlyk/Code/ovrcr/.worktrees/codex-reporting`. No staging, commits, pushes, live configuration, credentials or native GUI actions. Coordinator-owned plan and research changes were left untouched. Final delivery/work diary and release acceptance remain coordinator-owned.

## Changed files

- `src/cli/codex_setup.rs` (new): print-only bounded TOML composition; exact native direct-exec five-hook schema; idempotent exact reporter recognition; preservation of other values/handlers/order; version/config-only doctor with sanitized diagnostics and explicit unverified trust/delivery. Unit test for strict configuration recognition.
- `src/cli/agent.rs`: explicit Codex/Claude setup and doctor dispatch; executable default derives from selected provider.
- `src/cli/args.rs`: allow Codex setup/doctor; optional executable override.
- `src/cli/mod.rs`: register Codex setup module.
- `src/cli/agent_setup.rs`: share existing shell quoting, executable path and signal-protected version-probe cleanup; Claude behavior retained.
- `src/report/admission.rs`: expose existing bounded probe to CLI diagnostics (coordinator informed and approved minimal exposure).
- `Cargo.toml`, `Cargo.lock`: root consumes already existing workspace TOML dependency; no new crate/version.
- `tests/agent_setup.rs`: three public Codex integration tests covering schema, values/trust/approval handlers/order, repetition, spaced/apostrophe executable, actual emitted no-op command, provider defaults and version dispatch, unsupported/missing executable, sanitized unexpected output/config error, conservative hook recognition and no server writes.
- `tests/cli.rs`: public help regression for provider selection and removal of misleading fixed Claude default.
- `docs/codex-reporting-setup.md`: current print/install/doctor instructions, exact native schema/grammar, trust-before-first-tracked-prompt, empty-success no-op, semantics and first-release exclusions, retained broader source blocker.
- `docs/agent-reporting-support.md`: separate approved last-observed-turn milestone from blocked continuous-identity/metrics scope, support planned pending Task 4.
- `docs/cli-reference.md`: Codex setup/doctor entries.
- This report (ignored worker artifact).

## Verification and retained failures

All commands below used `rtk proxy`; Rust commands used `CARGO_INCREMENTAL=0`. No test filter reported zero tests.

1. `rtk proxy env CARGO_INCREMENTAL=0 cargo test -p ovrcr --test agent_setup codex -- --nocapture` — exit 101, **2 passed, 1 failed, 7 filtered**, 2.07s. Failure: `codex_setup_preserves_handlers_trust_and_quotes_noop_helper` asserted `{}` plus newline but actual stdout was empty (`left: []; right: [123,125,10]`). This was an incorrect new expectation, not a product defect: inspected `src/cli/report.rs` and coordinator confirmed native Task 2 empty-success no-op. Corrected docs/output/test to empty successful response without modifying Task 2 helper behavior.
2. `rtk proxy env CARGO_INCREMENTAL=0 cargo test -p ovrcr --test agent_setup -- --nocapture` — sandbox exit 101, **8 passed, 2 failed, 0 filtered**, 5.40s. `doctor_inspects_unbound_and_unavailable_sessions_without_private_values` failed socket bind with OS1 PermissionDenied / Operation not permitted; `doctor_reports_partial_config_proof_missing_and_unsupported_versions_without_secrets` observed unavailable instead of supported during concurrent probes. All three Codex tests passed. No assertions weakened or tests skipped.
3. `rtk proxy env CARGO_INCREMENTAL=0 cargo test -p ovrcr --test agent_setup -- --nocapture --test-threads=1` — required process/socket permission, exit 0, **10 passed, 0 failed, 0 ignored, 0 filtered**, 7.06s. Signal cleanup retained real ESRCH proof for SIGINT, SIGTERM, SIGHUP and SIGQUIT. This rerun resolves the functional gate; the prior concurrent sandbox failure remains above rather than being claimed as an application regression.
4. `rtk proxy env CARGO_INCREMENTAL=0 cargo test -p ovrcr --test cli codex_setup_and_doctor_help_expose_provider_dispatch -- --nocapture` — exit 0, **1 passed, 31 filtered**, 0.63s.
5. `rtk proxy env CARGO_INCREMENTAL=0 cargo test -p ovrcr --test cli -- --test-threads=1` — required process/socket permission, exit 0, **32 passed, 0 failed, 0 ignored, 0 filtered**, 17.52s. Includes existing managed hook, deadline, native argv, Claude doctor, response-ready CLI, offline and registry tests.
6. `rtk proxy env CARGO_INCREMENTAL=0 cargo test -p ovrcr --bin ovrcr codex_setup -- --nocapture` — exit 0, **1 passed, 0 failed, 0 filtered**. New unit test rejects wrong command, filtered/non-string matcher, true/non-boolean async; accepts retained SessionStart matcher only for SessionStart.
7. `rtk proxy env CARGO_INCREMENTAL=0 cargo clippy -p ovrcr --bin ovrcr --test agent_setup --test cli -- -D warnings` — exit 0. Executed before the final unit-only addition; no production changes afterward.
8. `rtk proxy env CARGO_INCREMENTAL=0 cargo fmt --all -- --check` — exit 0, no output.
9. `rtk proxy git diff --check` — exit 0, no output.
10. `rtk proxy git rev-parse HEAD` — `26be5eb8ba2728135728192591283a2c906324de`.

The test-file append shell invocation inadvertently included a trailing bare `rtk`, which printed RTK help and returned exit 2 after the file append succeeded; no source/test operation failed from that command. The subsequent compiler/test runs verify the appended content.

## Self-review and limits

- Setup uses retained TOML command schema, not hooks.json. It never writes the supplied file; the integration test compares original bytes and parses preserved unrelated handlers, project trust and hook-state values. Composition may reformat TOML/drop comments, disclosed explicitly. Existing trust values are preserved, never synthesized or interpreted as proof. An old reporter at a different executable path needs explicit removal, documented.
- Absolute executable quoting is reused from Claude and tested through the actual emitted shell command for all five hooks outside managed launch. Each exits successfully with empty stdout, with no approval result or server creation.
- Doctor probes only `--version`, uses existing bounded cleanup and sanitizes arbitrary probe output. It neither calls inspect nor reads provider credentials. Optional Codex `--session` reports not-inspected and points to session usage; no implicit inherited session/server dependency.
- Native root matching Stop means response available to review, not task success or settled completion. Docs call out other native Stop hooks, Ctrl-C versus earlier Escape evidence, missing ending hooks/API errors, next-prompt rebinding and reconnect limits. Raw Codex launches are explicitly untracked.
- No original-defect RED run against baseline was attempted; Task 3 adds a previously unavailable CLI provider path. The retained new-test failure was an expectation correction, not RED evidence for a bug fix.
- Full workspace/all-feature, Linux, capacity and native GUI release gates were not run by this bounded worker. Task 4 remains necessary. Existing native Task 2 evidence informed schema/semantics only.

## Default-threading fixture correction at c3b8faf

Coordinator requested a test-only correction after the all-feature/default-parallel workspace run at `c3b8faf` reproduced startup contention. Original full failure remains unchanged at `research/codex-response-ready-acceptance/final/local-workspace-attempt-1.log`: agent_setup **7 passed, 3 failed**, exit 101, 3.00s. Codex default dispatch and the existing Claude supported-version check each received `unavailable` instead of `supported`; the signal-cleanup fixture's owned probe did not publish readiness. This supersedes the earlier narrower inference that a permitted serial rerun alone resolved the default workspace gate.

Correction changes only `tests/agent_setup.rs`: apply the existing `tests/server_lifecycle.rs` poison-tolerant `env_lock` pattern locally to all ten setup/doctor subprocess fixtures. Each fixture acquires the mutex before constructing or launching temporary executables, so every caller competing for these bounded probe cold starts is coordinated. No assertion, test case, production code, one-second probe deadline, or signal cleanup contract changed. Poison recovery allows subsequent test assertions to run after a failing fixture. This suite now tests reporting/setup/cleanup contracts under coordinated fixture startup; it makes **no concurrent provider-launch throughput claim**. This is a test-resource ruling authorized by the coordinator, not a global cargo `--test-threads` workaround.

Verification on baseline `c3b8faf` plus the test-only diff:

- `rtk proxy env CARGO_INCREMENTAL=0 cargo test -p ovrcr --test agent_setup --all-features -- --nocapture` — required isolated socket/process permission; **default cargo test threading**, no test-thread override; exit 0; **10 passed, 0 failed, 0 ignored, 0 filtered**, 7.48s. Actual all-feature binary `target/debug/deps/agent_setup-1922858abc145aff`. Supported/unsupported/default dispatch assertions all executed. Cleanup evidence: SIGINT PGID48569, SIGTERM PGID49200, SIGHUP PGID50368, SIGQUIT PGID50532 each reported `absent=ESRCH` and the expected doctor termination signal.
- `rtk proxy env CARGO_INCREMENTAL=0 cargo fmt --all -- --check` — exit 0, no output.
- `rtk proxy git diff --check` — exit 0, no output.
- `rtk proxy git diff -- tests/agent_setup.rs` — self-review confirmed one local mutex/helper/comment plus one guard at the beginning of each of all ten tests; every prior assertion unchanged.

No staging, commits, production changes or changes to coordinator-owned plan/research evidence. Full workspace rerun and independent review remain coordinator-owned.

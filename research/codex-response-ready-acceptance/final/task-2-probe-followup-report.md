# Shared probe fixture follow-up

Baseline: `2db1897409bead1e1221c9f508575cabe6564844`.

Only `src/report/admission.rs` changed, inside `initial_admission_probe_cleans_descendants_after_leader_exit`. The fixture now runs the existing `/bin/sh -c` interpreter with positional arguments instead of executing a freshly written script. It still invokes the production bounded `probe_command` followed by the same `classify_version` mapping used in `pinned_version`.

All five cases remain: supported2.1.267/2.1.268, unsupported2.1.266/2.1.269, nonzero2.1.268 exit. Assertions on supported status, exact observed version, real leader/descendant identity, process-group disappearance, and task-owned cleanup remain. The production one-second timeout and code are untouched.

The failing full-workspace attempt is retained at `research/codex-response-ready-acceptance/final/local-workspace-attempt-2.log`: root library38passed/1failed, observedNone rather thanSome("2.1.266"). Prior retained diagnostic attempt22 demonstrated that newly written executable startup can consume the complete probe budget. The full-workspace failure did not retain its exact failing syscall, so this report does not claim that syscall was individually proven. The fixture now isolates the lifecycle/classification behavior it tests from that executable-start overhead, matching the earlier direct-shell blocked-probe fixture.

Verification:

- `rtk proxy env CARGO_INCREMENTAL=0 cargo test -p ovrcr --lib --all-features`: exit0,39passed/0failed, normal test parallelism (1.52s). Full log: sibling `task-2-attempts/probe-followup-1.log`.
- `rtk proxy cargo fmt --all -- --check`: exit0.
- `rtk proxy env CARGO_INCREMENTAL=0 git diff --check`: exit0; `task-2-attempts/probe-followup-2.log`.
- `rtk proxy env CARGO_INCREMENTAL=0 cargo test -p ovrcr --lib`: exit0,30passed/0failed, normal test parallelism (1.01s). Build first waited for the shared directory lock; log `task-2-attempts/probe-followup-3-default-lib.log` retains that wait and result. No additional suites ran.

Self-review: no production changes; no assertions dropped, relaxed, or retried; no timeout extensions. Only fresh executable-file creation/mode changes were replaced by direct invocation of an existing interpreter. The test still owns and verifies actual native process-group side effects. No staging or commits; coordinator owns exact-revision review.

SHA256 of updated source: `35f0dafcdf4341ab840c7ec9d7a58089a03ce9c2998764a1925d552f63a89ed6`.

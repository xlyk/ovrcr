# Version probe failure diagnosis

Both retained failures occurred at the same production boundary: `pinned_version`
returned `false` for a task-owned executable that prints the exact supported
`2.1.267 (Claude Code)` line.

- The earlier admission-unit failure was at
  `initial_admission_probe_cleans_descendants_after_leader_exit`,
  `src/report/admission.rs:1388:13` on revision `ce75087`: left `false`, right
  `true`. This assertion precedes the descendant-cleanup checks, so that run did
  not demonstrate a cleanup leak. The failed unit filter finished in 1.39s. Its
  exact filtered rerun finished in 0.39s and passed 1/1; the roughly one-second
  difference is consistent with the probe's one-second bounded failure path,
  but the retained output does not identify which low-level branch reached it.
- The full workspace gate's `tests/agent_setup.rs:191` failure expected
  `probe_status=supported` and received `unsupported_or_unavailable`. That JSON
  value is selected directly from the same `pinned_version` boolean. The earlier
  admission probe passed in the same full workspace run before this later doctor
  fixture failed.
- `git show ce75087^:src/report/admission.rs` and
  `git show ce75087:src/report/admission.rs` show the `pinned_version`
  implementation is unchanged by the resume feature commit.
- A focused `CARGO_INCREMENTAL=0` doctor test invocation passed 1/1 in 0.71s,
  and a focused version-probe test invocation passed 1/1 in 0.45s. These are
  observations, not proof of the earlier failures' cause.
- The failed full run occurred before disk cleanup, and multiple tests execute
  subprocess probes concurrently by default. The retained evidence does not
  prove that disk availability, scheduling, or concurrency caused either false
  negative. No timeout, assertion, or production probe behavior was changed.

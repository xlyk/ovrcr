# Failed-attempt resource cleanup audit

Scope: existing logs and source only. No process inventory, liveness probe, signal, test, build, UI, network operation or source change was performed. This report does not claim an observed live leak or propose an unrelated product correction.

## Cargo log 49: runtime ownership-listing test

`49-macos-workspace-final.log:2377-2388` records `listing_failure_pause_kill_close_are_ownership_uncertain_and_do_not_mark_stopped` failing its close error-code assertion at `server/tests.rs:5195`: Conflict instead of OwnershipUncertain. The test binary completed 437 passed / 1 failed / 1 ignored. The prior pause/kill iterations reached their explicit cleanup; the close iteration panicked before `set_listing_error_hook(None)` and `cleanup_test_session` at the end of the loop (`server/tests.rs:5174-5209`).

`spawn_live_test_session_with_hook` (`server/tests.rs:2741-2784`) launches a real PTY session with `sh -c "trap '' HUP TERM; while :; do sleep 1; done"`, logical session ID 92 for the close iteration. That ID is not an OS PID/PGID. No actual PID, PGID, TTY or private cwd for this iteration appears in the failure body. Its retained registry is in memory (`test_state_with_dispatch`, lines 2636-2678), not a durable per-process inventory.

Cleanup ownership on the panic path:

- `TestSessionEvents` has cancellation/finish methods but no Drop implementation. Dropping its JoinHandle detaches the event consumer; its closure retains an Arc<Session> and loops until cancellation/Exited/disconnect (`server/tests.rs:2810-2868`). The panic does not set cancellation.
- Session spawning gives reader and child-waiter threads their own Arcs (`session/mod.rs:696-720`). The waiter waits for the real child and then group absence; it does not initiate termination (`session/io.rs:36-72`).
- Neither Session nor ServerState has a terminating Drop implementation. Dropping the test’s state removes one session-map reference; it does not call shutdown/terminate. The explicit cleanup helper would terminate the session and finish/cancel the event consumer, but was bypassed (`server/tests.rs:2887-2910`).

The returned Conflict does not itself certify group exit: `ServerState::close_terminal` has early conflict/fence checks before `session.terminate` and another target check afterward (`server/mod.rs:603-648`). The log prints only the error code, so it does not show which branch or whether any signal was delivered.

Consequently cleanup is **not guaranteed**, including after the whole test binary exits. Binary exit closes its PTY descriptors and ends threads, but does not itself signal the child’s separate process group with SIGKILL; the fixture explicitly ignores HUP/TERM. The process group can therefore outlive the binary. This is a source-supported possibility, not proof it happened or remains alive.

## Nextest log 60: doctor version-probe attempt

`60-macos-nextest-ci.log:164-186` records TRY 1 FAIL at `tests/agent_setup.rs:438`, `owned probe did not publish readiness`, followed by TRY 2 PASS. The first attempt explicitly recorded ESRCH for probe PGIDs 14718 (SIGINT) and 14765 (SIGTERM). Those two groups have direct absence evidence. The failure is in the following SIGHUP iteration, before its probe PID was obtained; the SIGQUIT iteration was not reached in that attempt.

The readiness-timeout branch explicitly kills/reaps only the doctor direct child (`agent_setup.rs:435-438`). It has no test-owned RAII process-group guard and no known probe group yet. TempDir cleanup removes the per-iteration script/marker directory, not processes. Later timeout/assertion branches do have a known group and attempt group SIGKILL, but they were not reached here.

The ordinary doctor path blocks INT/TERM/HUP/QUIT while `probe_version` owns a child, then restores signals (`src/cli/agent_setup.rs:666-692`). `probe_command` creates a separate process group, has a one-second deadline, and group-kills/reaps on normal completion/error/timeout (`src/report/admission.rs:234-315`). The test’s readiness-timeout `doctor.kill()` is SIGKILL and can bypass this ordinary cleanup if a probe was already spawned. If the doctor had not spawned a probe yet, there is no group to leak. The log does not distinguish those cases. A spawned script eventually executes `sleep 60`, so an orphan may also have exited naturally later; that is not an absence receipt.

TRY 2 PASS proves that retry’s own assertions, not cleanup of the first attempt’s unrecorded third group. Report this as a retried/flaky case, not a clean first-pass result.

## Bounded ownership recovery

Prefer any **already captured** OS PID/PGID plus birth/start identity, parent chain, TTY and unique fixture cwd/path from the original Cargo 49 / Nextest attempt. Cross-check against the completed owning test process and its exact run interval before any action. For the runtime case, logical ID 92 and the generic shell command are insufficient. For doctor, the two logged groups already have ESRCH proof; no third identity was logged. Recovering the exact temporary `owned-probe` marker/script path or an original child-spawn receipt would be stronger than command-name matching.

The private task HOME/build path can corroborate ownership, but cannot alone authorize group removal while the current CI run uses the same task environment. The runtime fixture explicitly removes hook/session identity variables for this no-hook spawn (`session/mod.rs:556-568`), so do not assume `OVRCR_SESSION_ID=92` will exist. Its TempDir may have been deleted during unwind while a child retained the cwd.

If existing receipts cannot bind an exact surviving group to the failed attempt, leave actual absence unqualified. Do not infer cleanup from test-binary exit, retry success, deleted directories or another test’s successful teardown; never kill by shell/command substring. Any later recovery must preserve currently running test resources and revalidate the original owned PID/group identity immediately before signaling. No such recovery was performed here.

## Source-only attribution follow-up

This appendix identifies fields the spawn paths preserve; it does **not** establish that any candidate exists or that a particular inherited variable was present. No process inspection or signal was performed. All macOS Cargo runs have ended according to the parent; CI 60’s doctor retry passed. Those statuses do not establish absence of either earlier attempt’s unrecorded child.

### Runtime loop

- Exact supplied argv is `["sh", "-c", "trap '' HUP TERM; while :; do sleep 1; done"]` (`crates/ovrcr-runtime/src/server/tests.rs:2741-2770`). The executable is PATH-resolved by portable-pty, which explicitly preserves supplied `argv[0]` as `sh`; a canonical executable path is not fixed by the fixture. The shell remains the loop leader; its recurring `sleep 1` child has a different argv and is not by itself distinctive.
- The helper assigns a fresh `tempfile::tempdir()` as cwd. Session spawning passes that path to `CommandBuilder::cwd` (`session/mod.rs:534-539`). The private path can disappear during unwind while a surviving process still references its deleted cwd. The source does not recover this failed iteration’s random path, and the failure body does not print it.
- Cargo.lock pins portable-pty 0.9.0. Its `src/cmdbuilder.rs:74-89` copies all parent `vars_os`; `as_command`, lines 498-539, reconstructs that stored environment after `env_clear`. This is not an allowlist. Consequently HOME, CARGO_MANIFEST_DIR and any NEXTEST_* variables survive **if present in the test process**. Session spawning removes restore/cloud variables and five reporting/session capability variables (`session/mod.rs:540-568`), but does not remove HOME/CARGO/NEXTEST. `shell_prompt::configure` immediately returns for this explicit three-argument `sh -c` invocation (`session/shell_prompt.rs:8-15`); it does not rewrite HOME. The no-hook child has no OVRCR_SESSION_ID supplied by this fixture.
- portable-pty’s Unix pre-exec calls `setsid()` and configures the controlling TTY (`src/unix.rs:235-275`), making the initial shell PID=PGID=SID. OVRCR additionally checks PGID equals the child PID (`session/mod.rs:588-599`). Group/session/TTY identity is therefore useful corroboration, but no actual values for logical session 92 were logged.

For the old Cargo 49 loop, an exact argv match plus a unique recorded task/workspace environment, orphan PPID 1, private/deleted fixture cwd, PTY/session-leader identity and a birth time within that failed Cargo attempt would be stronger together than HOME or argv alone. A birth time before CI 60 helps exclude resources created by the later run; it does not establish ownership by itself. No source evidence establishes that the older Cargo process carried CI 60’s NEXTEST_RUN_ID.

### Doctor third probe

- The test executes compiled `env!("CARGO_BIN_EXE_ovrcr")` with argv `agent doctor claude --json --executable <fresh-temp-root>/claude` (`tests/agent_setup.rs:19-31,404-429`). The helper sets OVRCR_HOME to that exact root and OVRCR_SOCKET to `<root>/socket`; it removes only the five reporting/session variables, without env_clear or current_dir. HOME, CARGO_MANIFEST_DIR and any NEXTEST_* entries are inherited if present. Its cwd therefore inherits the test process’s cwd; the fixture does not set it to the private root.
- The unique executable contains `#!/bin/sh`, then `printf '%s' "$$" > '<root>/owned-probe'`, then `exec sleep 60` (`tests/agent_setup.rs:409-420`). `probe_version` starts the supplied `<root>/claude` executable with `--version` (`src/report/admission.rs:222-226`). After the script’s exec, the process retains its PID/group/environment but its visible argv becomes the shell-resolved `sleep 60`; the original script path and `--version` are no longer reliable argv requirements. The exact resolved sleep executable path is not fixed in source.
- `probe_command` uses `.process_group(0)` and removes only those same five reporting/session variables (`src/report/admission.rs:234-251`). It preserves the unique OVRCR_HOME and OVRCR_SOCKET, plus inherited HOME/CARGO/NEXTEST if supplied. It has no current_dir override and no setsid call: probe PGID=PID, while its SID is inherited. Do not apply the runtime PTY leader’s SID=PID predicate to this probe.
- A successfully written `<root>/owned-probe` contains the probe PID, also used as PGID by this test. `<root>/claude`, `<root>/owned-probe`, OVRCR_HOME and OVRCR_SOCKET share one unique random root. The third readiness failure prints neither that root nor its PID; TempDir unwind may remove both files. There is no separate durable PID file in this fixture. The two printed earlier groups have their own ESRCH proof and do not identify the third.
- `60-macos-nextest-ci.log:3` records run ID `264abf53-7a71-4ea9-9e05-d8b7a2aefe26`. Source shows NEXTEST_* preservation, not a captured environment for the failed child. A later candidate’s matching run ID/test identity would need independent observation; this log alone cannot supply its missing PID/root. A doctor candidate’s inherited cwd need not be private/deleted, so requiring that particular runtime-loop trait would miss this spawn path.

The strongest available source-backed doctor predicate would combine the unique OVRCR_HOME/OVRCR_SOCKET root (or an original readiness marker), exact current helper argv appropriate to its exec stage, matching recorded run/test identity if actually present, orphan parent status and birth within TRY 1’s interval. None of these are newly observed here. Matching HOME, generic `sleep 60`, a deleted directory, or retry success alone remains insufficient attribution.

## Bounded host attribution receipt (no signals)

```json
{
  "observed_utc": "2026-10-31T10:40:00.627126+00:00",
  "method": "one successful PID/PPID snapshot and one kern.procargs2 read per PID; only allowlisted env entries retained and emitted; exact helper argv candidates only; per-candidate ps identity/lsof cwd reads; no signals",
  "earlier_sandbox_attempt": "ps denied before PID enumeration; successful pass uses approved host read access",
  "ci_anchor_pids": [
    12236
  ],
  "ci_anchor_metadata": [
    {
      "pid": 12236,
      "ppid": 12235,
      "pgid": 12235,
      "sid": 12235,
      "birth_and_tty_ps": "Sat Oct 31 03:23:23 2026     ??",
      "cwd": [
        "/Users/xlyk/.codex/worktrees/architecture-survey/ovrcr"
      ],
      "environment_allowlist": {
        "HOME": "/private/tmp/ovrcr-384-evidence/home"
      }
    }
  ],
  "excluded": {
    "runtime_active_ci_descendants": 0,
    "doctor_active_ci_descendants": 0
  },
  "arg_read_errors": {
    "EINVAL": 285
  },
  "log_time_bounds": {
    "49-macos-workspace-final.log": {
      "created_utc": "2026-10-31T09:36:58.991045+00:00",
      "mtime_utc": "2026-10-31T10:15:58.886241+00:00",
      "note": "file creation/last write bounds, not per-test start/end timestamps"
    },
    "60-macos-nextest-ci.log": {
      "created_utc": "2026-10-31T10:23:23.978199+00:00",
      "mtime_utc": "2026-10-31T10:39:55.781685+00:00",
      "note": "file creation/last write bounds, not per-test start/end timestamps"
    }
  },
  "runtime_candidates": [],
  "orphan_doctor_candidates": []
}
```

Interpretation: this single successful host snapshot found no readable exact runtime-leader argv and no orphan doctor-helper stage with the fixture root/environment predicate. The active task CI process was PID 12236, born 10:23:23 UTC; its descendants were excluded by ancestry. No candidate therefore met the requested old-runtime conjunction (PPID 1, exact argv, task environment, private/deleted cwd, PID=PGID=SID, birth before CI 60), and there is no attributed group for cleanup. This is not a universal absence proof: 285 process-argument reads returned EINVAL, and processes can exit or change between reads. No opaque/read-error process was treated as owned, no substring match was used as ownership, and no signal was sent. The initial sandboxed attempt stopped at denied ps before enumeration; the one successful host pass used approved read access. No further process pass was performed.

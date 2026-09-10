# Task 1 independent review

Decision: changes requested. No production-code defect was demonstrated; the blockers are incomplete required ownership and cleanup proofs.

## Blocking findings

1. **[P2] Record and verify collector ownership before killing the supervisor** — `tests/server_lifecycle.rs:7326` and `:7517`. The test waits for collected metrics, but only records the outer session and native groups. The collector is a real separate process group (`src/report/collector.rs:72`), so the final assertion over `fixture.process_groups` excludes it. A collector that survives supervisor death indefinitely would still pass this test. Task 1 explicitly requires recording supervisor/native/collector ownership and proving owned processes/groups absent. Capture the attributable collector PID/PGID before SIGKILL, register it for failure cleanup, and explicitly verify its fate and final absence. Record the required foreground TPGID with the ownership evidence as well.

2. **[P2] Clean the killed supervisor's private callback directory** — `tests/server_lifecycle.rs:7356` (SIGKILL) and `:7496` (cleanup). `InvocationChannel::new` allocates its `ovrcr-a-*` TempDir independently of the ControlFixture root (`crates/ovrcr-runtime/src/agent_runner.rs:267`). SIGKILL skips that object's destructor. The native helper reads `OVRCR_AGENT_SOCKET`, but the test never retains that owned path for cleanup and never removes its socket/directory. Removing the main fixture root therefore leaves this separate task-owned resource behind on every successful crash run. Persist the safe socket path in fixture evidence before readiness, explicitly remove that exact owned directory after its processes are gone, and assert path absence. Do not discover ownership by sweeping all matching temporary directories.

## What the assertions do establish

- A real supervisor PID alone receives SIGKILL after root binding and retained metrics; the outer shell and native fixture remain present, and the server reports `supervisor_disconnected` with Unavailable health and non-Complete retained usage.
- A replacement invocation reserves and binds the original runtime session without an explicit release from the killed supervisor. Old-binding report and old-lease release return errors; the replacement binding and Connected health remain intact.
- The exec helper checks open descriptors using fcntl/fstat and Unix socket local/peer path identities, verifies PTY stdio, performs a real private callback that binds the expected conversation, and exchanges actual terminal input/output. The descriptor enumeration is capped at 65,536, so its proof covers that fixture range rather than arbitrary inherited descriptors above the cap.
- The new AdmissionProxy fields only capture Reserved/Bound responses; existing fault matching and forwarding paths are unchanged. No shared-proxy regression found by inspection.
- sockaddr field offsets are computed from the platform layout, supporting macOS/Linux differences without `/proc`. Linux execution remains unverified here.

## Exact revision and verification

Reviewed HEAD: `d041e49269c4eab9da48193a82ece2c851d0da4c`.
Base: `fa4e12dc36c418cfc7af2079d62c4e72d38edd1c`.
Diff: tests and retained Task 1 evidence only; no production edits. The unrelated untracked `research/claude-reporting-acceptance/part3-version/` directory was preserved.

Independent macOS checks, with escalation for isolated sockets/PTYs:

```text
rtk proxy env CARGO_INCREMENTAL=0 cargo test -p ovrcr --test server_lifecycle agent_run_supervisor_sigkill_releases_reporting_watch -- --exact --nocapture
exit 0; 1 passed, 0 failed, 0 ignored, 77 filtered out; 1.91s
fixture root: /var/folders/j9/h6rlffxd1r794s9g9ps_52t40000gn/T/.tmpThRcxK

rtk proxy env CARGO_INCREMENTAL=0 cargo test -p ovrcr --test server_lifecycle agent_run_native_exec_does_not_inherit_supervisor_lease -- --exact --nocapture
exit 0; 1 passed, 0 failed, 0 ignored, 77 filtered out; 1.51s
fixture root: /var/folders/j9/h6rlffxd1r794s9g9ps_52t40000gn/T/.tmpjFQUty
```

Both named main fixture roots were separately verified absent after the tests. The crash test does not expose the separate private callback directory, so its cleanup could not be verified or safely performed by this review. Uncertain-ownership temporary directories were preserved. The active GUI and loopback receiver were not touched.

Existing retained macOS successes were also inspected. No Linux run, broad regression suite, or native provider acceptance was performed for this review. The required Linux results remain an acceptance gap, not a pass. No production changes, staging, or commits were made. The coordinator owns the substantive work-diary closeout.

After the review tests completed, another worker began modifying `tests/server_lifecycle.rs` (46 added lines observed). Those subsequent uncommitted changes are outside this review; all finding line numbers refer to the committed d041e49 version.

## Correction review: fcf26fe

Reviewed exact correction `fcf26febf9672c62ed8cb1de73b0f3dca3c05f18` against `d041e49269c4eab9da48193a82ece2c851d0da4c`.

Both blocking findings are resolved. No new blocker found in the correction diff.

- The collector is identified before SIGKILL by the known supervisor PPID and exact `__agent-collector` argv token. The test requires exactly one matching child and PID equal to PGID, registers that owned group for cleanup, and requires ESRCH-backed group absence after supervisor death. This closes the previously omitted collector cleanup assertion.
- The native helper records its actual private callback socket path before readiness. After the native group disappears, the crash test removes that exact recorded callback directory, checks the expected directory-name prefix, and asserts path absence. The removal is based on attributable fixture evidence rather than a global temporary-directory sweep.
- `ps -axo pid=,ppid=,pgid=,command=` is available on macOS and Linux. No Linux-only `/proc` dependency was introduced. Linux execution still requires its own result.

Independent correction verification on macOS, with the same exact commands shown above:

```text
agent_run_supervisor_sigkill_releases_reporting_watch
exit 0; 1 passed, 0 failed, 0 ignored, 77 filtered out; 1.77s
fixture root: /var/folders/j9/h6rlffxd1r794s9g9ps_52t40000gn/T/.tmpUhL1Fi

agent_run_native_exec_does_not_inherit_supervisor_lease
exit 0; 1 passed, 0 failed, 0 ignored, 77 filtered out; 1.55s
fixture root: /var/folders/j9/h6rlffxd1r794s9g9ps_52t40000gn/T/.tmp25M4v7
```

Both main fixture roots were independently checked absent afterward. The crash run itself now asserts the private callback directory is absent. Retained coordinator correction logs were inspected and agree with the independent results.

Review status: the two requested code/test corrections pass review. This is not full cross-platform Task 1 acceptance: Linux is unrun here. The global plan's foreground TPGID evidence is still not recorded by these tests; do not claim that evidence from these results. Historical private callback directories from pre-correction runs remain uncertain ownership and were not swept. Only this report was appended; no production/test edits, staging, or commits were performed by the reviewer.

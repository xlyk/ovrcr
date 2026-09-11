# Initial admission review corrections

Review base: `de8cd87d756a89a6317e0f1b85778e3db8bc1845`. These are the two independently identified blockers only. Source verification showed signal handlers already precede the version probe; the coordinator withdrew the additional pre-handler concern, and no signal scope was added.

## P1: close pending admission within the current callback

Clear/branch now permanently closes local admission, resolves a pending Bind by the original operation's status immediately, and publishes Unavailable health using the same callback deadline. It never issues another Bind or depends on a later startup. A healthy acknowledgement keeps the lease until native completion.

The coordinator explicitly approved bounded failure behavior: if status or Unavailable health cannot be acknowledged, shutdown the watched socket before dropping the lease. The existing generation-safe disconnect cleanup marks runtime health Unavailable and relinquishes reporting ownership; native continues untracked. Socket shutdown makes graceful Drop release fail immediately rather than acquiring another one-second wait after callback expiry. No finalization, completion, new generation or reopening is fabricated.

The real CLI/private-hook/runtime tests use an isolated frame proxy. One case commits Bind but withholds its reply, then sends clear with no later startup; another rejects the subsequent status; another withholds the Health request/reply until the callback expires. Assertions observe actual runtime Unavailable state, unchanged binding/generation and synthetic activity revision, no reopening from the old invocation, successful healthy lease retention, and reporting-failure fallback before allowing a separate explicit reservation. Timeout closure is observed within 1.4 seconds, including test-process observation allowance; production callback work retains its 800 ms deadline.

## P2: clean the probe group before reaping its leader

The version probe uses waitid with WNOWAIT to observe leader exit while retaining its ownership anchor. Both successful and failed exits kill remaining owned group members before Child::wait reaps the leader. Timeout/error cleanup retains the same ownership boundary. A real executable fixture exits with status 0 and 1 while leaving a sleeping descendant; both groups are verified absent after the probe returns. Native argv and version eligibility are unchanged.

## Attempts and final checks

- 15: actual P2 behavioral RED, descendant remains after successful leader exit. The test kills its leaked owned fixture group before reporting failure.
- 16–22: retained test-fixture failures while wiring the proxy address and disabling inherited nonblocking mode on accepted proxy streams. These did not reach the reviewed P1 behavior and are not product-failure evidence.
- 23: actual P1 behavioral RED, rejected Health publication leaves runtime Connected.
- 24: actual P1 behavioral RED, lost committed Bind reply followed by clear leaves runtime Connected without another startup.
- 25: admission library GREEN, 3 passed, including both real probe leader-exit cases.
- 26: intermediate lifecycle run, 4 passed/2 failed; the two failures were an assertion after a deliberately new reservation had cleared the prior snapshot. The corrected assertion checks old-invocation non-reopening before that new reservation.
- 27: final admission lifecycle GREEN, 6 passed/1 ignored helper (executed inside native PTYs), including lost-Bind closure, failed status, withheld Health timeout, normal clear/branch and argv/probe fallback.
- 28: root library, 6 passed. 29: focused launcher CLI, 3 passed.
- 30: root all-target/all-feature check passed. 31: matching clippy passed with warnings denied.
- 32: formatting/diff checks recorded separately.

No full workspace suite or new native Claude session was run. Linux/native GUI and provider activity/metrics/settling remain outside this correction.

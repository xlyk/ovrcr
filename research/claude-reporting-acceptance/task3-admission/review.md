# Initial-admission independent review

Original implementation: `de8cd87d756a89a6317e0f1b85778e3db8bc1845`.
Correction reviewed: `665831a2a43a82592591c41c883bc9de168237dc`.

Independent reviewer traced correction code and actual socket/PTY assertions. Both original blockers are resolved, with no introduced blocking defects found:

- Lost Bind followed by clear resolves the original receipt. Unacknowledged status or unavailable-health publication shuts down the watched connection inside the callback bound; acknowledged closure retains the lease.
- The version probe kills owned remaining process-group members before reaping its leader on successful and failed exits.

Verdict: ready for the initial-admission slice. Reviewer did not change files or rerun tests. Managed native evidence was captured afterward in attempt09. A separate read-only review accepted its scoped checkpoint without evidence flaws; it does not add provider accounting or platform claims. Remaining Task3/provider work remains incomplete.

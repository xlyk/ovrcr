# Integrated verification results

`source.txt` records reviewed merge revision
`d12abda8100bd15cde4f9740c004eafd2f691116`. The contemporaneous
`tracked-status.txt` captured before verification is empty.

The local macOS all-targets, all-features workspace run passed 633 tests with 0
failures and 11 intentional ignores across 24 test binaries. Workspace clippy
with all targets and all features passed with warnings denied, and formatting
passed. Each retained command exit file records status 0.

The workspace doctest command also exited 0, but `doc-tests.log` records zero
tests. It confirms that doctest discovery and compilation did not fail and adds
no behavioral test coverage.

The native macOS smoke from the same clean tracked revision passed two actual
responses, one generation-1 binding, the complete uncertainty labels at 62
columns, pane close, detach, reattach, exit, and owned-resource cleanup. Its
detailed evidence is in [`native-integrated/results.md`](../native-integrated/results.md).

For private PR 55, GitHub Actions run 34518312559 completed all three jobs
successfully at exact revision `d12abda`. The Linux suite passed 607 tests with 0
failures and 11 intentional ignores across 23 test binaries; Linux formatting
and clippy passed, while the doctest command exited 0 with zero tests. The hosted
capacity test passed once with 30,000 reports, 3,000 stale rejections, five failed
collectors, 4.574 ms p99 control latency, 9.607 ms maximum control latency, and
sampled RSS growth of 1,448,656 KiB. Its largest application queue peak was 51
of 64 entries; collector queues peaked at one item and drained to zero. All 50
helpers and all 50 PTYs were absent after cleanup. The runner recorded Linux
`somaxconn` as 4096. Independent artifact analysis is retained in
[`linux/hosted-34518312559/results.md`](../linux/hosted-34518312559/results.md).

The integrated local checks do not complete the broader Claude capability
milestone. Complete provider accounting, Confirmed settling, arbitrary child API
`StopFailure` isolation, complete child accounting, and native
null-current-context timing remain open. A later targeted
[native child run](../native-child/results.md) proved isolation for real child
tool failure and `SubagentStop` callbacks without changing the `d12abda` product.
The hosted run samples process RSS and application queues; it does not prove a full
heap maximum or include kernel socket-buffer memory. The earlier local Docker
interruption is historical, and its task-owned container and image still await
cleanup because restarting the shared service was not authorized.

PR 55 is a mergeable draft. It has not been merged or released; those actions
remain separate decisions.

# Hosted Linux capacity result

GitHub Actions run [34518312559](run-final.json) tested pull-request revision
`d12abda8100bd15cde4f9740c004eafd2f691116`. Both requested jobs completed
successfully: [linux capacity 103009225075](linux-capacity-job-final.json) and
[linux 103009225570](linux-job.json). Their unmodified final logs are retained as
[linux-capacity-job-final.log](linux-capacity-job-final.log) and
[linux-job.log](linux-job.log).

## Capacity acceptance

The uploaded artifact is `linux-capacity-34518312559-1`, artifact ID
`10168694058`, digest
`sha256:56e6138f6f3882f14e50cc3af26dd35bd77282446a57dee1486f67293ba735de`.
The GitHub artifact record is preserved in [artifacts.json](artifacts.json), and
the downloaded contents are under [artifact/](artifact/).

[test.exit](artifact/test.exit) is `0`. The raw [test.log](artifact/test.log)
shows that the exact ignored filter executed one test: one passed, zero failed,
zero ignored, and zero filtered out. The test completed 30,000 reports across 50
sessions in its 60-second reporting window, rejected 3,000 deliberately stale
reports, and exercised five collector failures.

The measured acceptance limits passed:

- Control latency used 2,813 samples. The p99 was 4,574 microseconds against a
  250,000-microsecond limit, and the maximum was 9,607 microseconds against a
  1,000,000-microsecond limit. The raw samples are in
  [latency.csv](artifact/load/latency.csv).
- Process-tree RSS was 120,760 KiB at the empty-50 baseline and peaked at
  1,569,416 KiB. Growth was 1,448,656 KiB against the 3,145,728-KiB limit. The
  91 samples are in [resources.csv](artifact/load/resources.csv).
- The 180 server queue samples had a maximum `peak_items` of 51, within the
  asserted limit of 64, and a maximum `peak_bytes` of 7,520. See
  [queues.csv](artifact/load/queues.csv).
- All 50 collector queue rows had `peak_items` at most 1 and `peak_bytes` at
  most 323. All 50 drained collector snapshots had zero pending items and
  bytes. See [collector-queues.csv](artifact/load/collector-queues.csv) and
  [collector-final.csv](artifact/load/collector-final.csv).
- All 50 accounting rows retained 65,536 identities and reported the expected
  `accounting_limit` diagnostic. See
  [accounting.csv](artifact/load/accounting.csv).
- The dashboard received 30,225 frames, including 70 output frames. All 50
  collectors were reaped with successful cancellation, and the test confirmed
  that all 50 owned PTY PIDs were absent after server shutdown. The recorded
  owned identifiers are in [owned-pids.csv](artifact/load/owned-pids.csv).

## Host provenance and Linux regression

The capacity job ran on x86-64 Azure-hosted Ubuntu with Linux
`6.17.0-1022-azure`. It used rustc 1.98.1 and Cargo 1.98.1, reported 8,127,868 kB
of host memory, and had `somaxconn=4096`. The raw records are
[uname.txt](artifact/uname.txt), [rustc.txt](artifact/rustc.txt),
[cargo.txt](artifact/cargo.txt), [meminfo.txt](artifact/meminfo.txt), and
[somaxconn.txt](artifact/somaxconn.txt).

The separate Linux job passed formatting, clippy, the workspace test suite, and
doc tests. Its raw log contains 607 passed tests, zero failed, and 11 ignored
across 23 workspace result groups. Doc tests truthfully executed zero tests in
each of five crates, with zero failures.

## Limits

This evidence certifies the assertions and sampled measurements implemented by
the isolated hosted Linux capacity test at the named revision. RSS is sampled
process-tree usage, not a full heap maximum. The synthetic reporting workload
does not certify native provider accounting completeness, provider resume or
fork behavior, native null-context timing, child Stop/failure routes, or macOS
GUI behavior.

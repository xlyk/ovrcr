# Hosted Linux memory-result review

Independent review date: 2026-09-10. Reviewed the retained `linux-memory`
artifact from GitHub Actions run `34543276357`, attempt 1. No local stress test
or provider process was run.

## Decision

**PASS for the bounded Linux collector fixture.** The exact ignored test executed
once, passed every source assertion, exited `0`, and retained its successful
cleanup record. These measurements establish the selected simultaneous
32 MiB raw-record plus 16 MiB charged-index case across 50 real collector
helpers, followed by sequential parsing. They do not establish a general heap,
kernel-memory, native-provider, macOS, or 50-way simultaneous JSON-parse bound.

## Provenance and execution

The [artifact revision](../part3-final/ci-34543276357/linux-memory-34543276357-1/revision.txt)
is `3538b2807f2e08e887fb82e9fbf970304a0500c0`. A read-only GitHub comparison
showed this is the workflow merge of reviewed source
`8166712186a1d1a199111a2ba0c0b844132918e1` into its target base. Both revisions
have the identical source tree `42099b7005c92c3b4c907214c2070eb76ad100c2`;
the merge added no file difference from `8166712`.

The [test log](../part3-final/ci-34543276357/linux-memory-34543276357-1/test.log)
records exactly one executed test,
`fifty_collector_raw_index_and_sequential_json_high_water`: 1 passed, 0 failed,
0 ignored, 1 filtered out, in 95.47 seconds. The retained
[`test.exit`](../part3-final/ci-34543276357/linux-memory-34543276357-1/test.exit)
is `0`.

The [preflight](../part3-final/ci-34543276357/linux-memory-34543276357-1/preflight.txt)
recorded 7,068,072 KiB `MemAvailable` against the 6,291,456 KiB requirement and
13,361,102,848 free bytes against the 536,870,912-byte disk requirement. The
runner reported 8,128,880 KiB total RAM, Linux 6.17 on x86-64 Azure, Rust 1.98.1,
and cgroup path `/system.slice/hosted-compute-agent.service`.

## Measured high-water

The retained [`memory.csv`](../part3-final/ci-34543276357/linux-memory-34543276357-1/memory.csv)
has 14,821 data rows: 285 sampled process-tree rows, 14,535 per-process rows
(51 processes per sample), and one final cleanup row. It consistently covers the
test process PID 4099 and 50 collector PIDs 4101 through 4150.

| Checkpoint | Sampled process-tree RSS | Growth from empty 50-helper checkpoint |
| --- | ---: | ---: |
| Empty 50 helpers | 576,252 KiB (0.550 GiB) | — |
| Full 16 MiB charged index in all 50 | 1,802,180 KiB (1.719 GiB) | 1,225,928 KiB (1.169 GiB) |
| Full index plus 32 MiB pending raw record in all 50 | 3,453,652 KiB (3.294 GiB) | 2,877,400 KiB (2.744 GiB) |
| Final sequential parse, helper 49 | **3,894,736 KiB (3.714 GiB)** | **3,318,484 KiB (3.165 GiB)** |

The final `parsed_49` row is the largest simultaneous sampled process-tree RSS
in the artifact. At that point the 50 collectors' current RSS values ranged from
77,432 to 77,840 KiB (about 75.6–76.0 MiB each); their current RSS sum was
3,881,660 KiB. The test process used 8,904 KiB current RSS and reported a
9,524 KiB `VmHWM`.

Each collector's kernel-reported `VmHWM` ranged from 109,200 to 109,772 KiB
(about 106.6–107.2 MiB). The sum of the 50 individual maxima is 5,472,588 KiB
(5.219 GiB), but those maxima occurred at different times and are **not** a
simultaneous aggregate peak. The sampled process-tree RSS above is the retained
simultaneous observation; it includes the short-lived sampling process and can
double-count shared resident pages in the ordinary summed-RSS manner.

## Assertions and cleanup

Because the exact reviewed source tree produced the passing test result, the run
establishes these fixture assertions together:

- all 50 real collector helpers started as their own process-group leaders;
- every helper acknowledged EOF at empty, exact 65,536-identity/16 MiB index,
  and exact 32 MiB unterminated-record phases;
- every indexed snapshot had Partial Conversation usage with 393,216 canonical
  input, 327,680 output, 131,072 cache-read, 65,536 cache-write, and 262,144
  reasoning-output tokens;
- after the newline, helpers parsed sequentially and each returned
  `accounting_limit` for the new identity while preserving the complete prior
  `UsageTotals`, 65,536 identities, and 16 MiB charged bytes;
- no early/stale EOF response could satisfy a later phase because each helper
  stopped with no request in flight after its acknowledged barrier;
- normal cleanup cancelled all 50 owned groups, checked every group for `ESRCH`,
  removed the task-owned source directory, and only then wrote the final
  `cleanup,all50_groups_and_source_absent,0,0,0` row. That row is the last row in
  `memory.csv`.

The 95.47-second runtime completed well inside the ten-minute source work budget,
100-second aggregate cancellation allowance, and 15-minute workflow backstop.
The exit record and final cleanup row are both present, so the timeout failure
mode identified in the first source review did not occur.

## Limits of this result

This is a kernel RSS observation for one Linux runner and one deliberately valid
JSON shape. It does not measure precise allocator allocations, shared-page
proportional memory, page cache, pipe/socket/PTY kernel buffers, thread stacks,
or every fragmentation pattern. `VmHWM` is per process and asynchronous; summing
it does not reconstruct a simultaneous maximum. The artifact has no runtime
`MemAvailable`, swap, or cgroup-limit series, so it does not prove unused-memory
headroom at the peak.

The fixture parses one pathological record at a time. It does not cover 50
simultaneous JSON expansions, maximal valid JSON expansion, dispatcher/dashboard
queue maxima, unbounded client connections, 50 Claude process trees, or macOS.
Its 3.165 GiB growth is separate from the unchanged earlier capacity test and its
3 GiB sampled-growth threshold; it must not be substituted for that test's
shell-PTY, reporting, queue, and control-latency claims.

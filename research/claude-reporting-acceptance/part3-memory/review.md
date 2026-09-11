# Task 8 independent memory-fixture review

Reviewed exact `87529055b56a5fc5093ee50ae0c059ff17c45aa5` against
`79cba1e28dac5941d2ff4eca1077dc26fb38d68a` on 2026-09-10. Scope was limited
to `.github/workflows/ci.yml`, `tests/claude_reporting_load.rs`,
`research/claude-reporting-acceptance/part3-memory/hosted-fixture.md`, the Task 8
investigation, and the production collector/accounting code needed to validate
the assertions. The ignored high-water test was not run locally.

## Blockers first

### [P1] The CI timeout can bypass the fixture's cleanup and exit-status evidence

`.github/workflows/ci.yml:151-160` gives the entire test step 15 minutes. The
fixture permits three separate 180-second EOF barriers
(`tests/claude_reporting_load.rs:870-928` and `1061-1093`), then as many as 50
sequential 15-second parse waits (`935-957`), followed by 50 cancellations with
individual two-second budgets (`960-963`). Those internal bounds total more than
23 minutes before source construction and sampling. A slow but still
within-contract run can therefore hit the workflow timeout first.

Runner termination can kill the shell before `PIPESTATUS` is copied to
`test.exit`, and it bypasses Rust unwinding, `CollectorController::drop`, the
explicit ESRCH checks, source-directory removal, and the cleanup row. The
always-upload step can retain partial files, but it cannot recover the missing
test exit status or prove fixture cleanup. Make one internal global deadline
expire with margin before the workflow step, or raise the step timeout above the
sum of internal bounds while retaining a job-level backstop. Hosted execution
should wait for this correction.

## Standards

One hard lifecycle-evidence issue: the outer timeout conflicts with the repository
testing rules requiring distinct exit status, bounded failure cleanup, and actual
absence checks for owned groups. This is the P1 issue above.

No other standards finding. The change is isolated to an ignored acceptance
fixture and a separate hosted job; it uses the existing collector controller,
real process groups, deterministic response barriers, task-owned paths, and
retained provenance rather than adding production instrumentation. The existing
`fifty_session_reporting_capacity` body and its 3 GiB sampled-growth assertion
are unchanged; the diff only appends the new fixture after it.

## Spec

### [P2] “Unchanged totals” is asserted for only two usage fields

After the complete 32 MiB record is parsed, the fixture checks retained identity
and charged-byte counts plus `input_tokens` and `output_tokens`
(`tests/claude_reporting_load.rs:940-945`). It does not compare the complete
`UsageTotals` value before and after the rejected new identity: scope, coverage,
cache-read, cache-write, and reasoning output are not part of the equality
assertion. The Task 8 requirement says totals remain unchanged. Capture the
pre-record `UsageTotals` and compare the full value after `accounting_limit`, or
assert every field explicitly. Using nonzero values for the optional components
would make that assertion meaningful rather than relying on their current zero
or unknown values.

The remaining requested fixture behavior is present in source:

- Preflight records and requires at least 6 GiB `MemAvailable` and 512 MiB free
  space on the source filesystem before any helper starts. A live 1 GiB
  `MemAvailable` stop is checked during barriers, sampling, and each parse.
- The fixture launches 50 real `CollectorController` children and verifies each
  child is its own process-group leader.
- The empty, index, and raw phases require actual `caught_up` responses. No
  request remains in flight after a barrier marks a helper ready, so the next
  phase cannot consume a stale EOF response. File writes are flushed before each
  barrier.
- Exactly 65,536 distinct 72-byte message/request identities charge 256 bytes
  each: 112 bytes of fixed key/value storage plus 144 identifier bytes, reaching
  exactly 16 MiB. Every helper must report that count and charge.
- The unterminated record body is constructed to exactly 32 MiB and includes 96
  nested array levels and 65,536 distinct object fields. Appending its newline
  only after the raw barrier leaves all other helpers without an in-flight read
  while each selected helper parses.
- Identity 65,536 is distinct from the indexed 0 through 65,535 range. Requiring
  `accounting_limit`, unchanged identity/charged counts, and retained nonzero
  input/output sums distinguishes successful parsing and admission rejection
  from record-limit, malformed-JSON, or silent-discard behavior.
- `memory.csv` uses a distinct `sampled_process_tree` row for summed sampled RSS
  and `process` rows for each current `VmRSS`/`VmHWM`; it does not sum individual
  high-water marks. The report accurately limits this to kernel-reported RSS
  observations and lists the remaining allocator, kernel, queue, native-process,
  macOS, and simultaneous-expansion gaps.
- The workflow compiles the exact test binary, records revision, kernel,
  toolchain, memory, disk, and cgroup provenance, invokes one exact ignored test,
  preserves the pipeline's Cargo exit status when the shell reaches that code,
  and always uploads the evidence directory. The P1 timeout path is the exception.
- Normal completion explicitly cancels all 50 owned groups, checks every PGID for
  ESRCH, drops the temporary directory, checks source-directory absence, and
  flushes a cleanup row. Assertion unwinding invokes the existing controller
  drop cancellation and `TempDir` cleanup, subject to the unproven absence check
  and the outer-timeout bypass described above.

## Verification status

Read-only checks confirmed both revisions resolve, the commit list is exactly
`8752905 test: add isolated collector memory high-water acceptance`, the
three-dot diff is nonempty and limited to the two implementation files plus the
hosted-fixture report, and `git diff --check 79cba1e...8752905` passes. The
worker-recorded compile, Clippy, rustfmt, and diff checks were inspected but not
rerun. Behavioral acceptance remains unrun as required.

**Summary:** Standards: 1 blocking lifecycle-evidence finding (P1). Spec: 1
partial assertion finding (P2). The hosted high-water run should not start until
the P1 timeout/cleanup/exit-status mismatch is corrected.

## Correction re-review: 8166712

Re-reviewed exact `8166712186a1d1a199111a2ba0c0b844132918e1` against
`87529055b56a5fc5093ee50ae0c059ff17c45aa5`. The intervening `dd08be0` commit is
documentation/evidence only; the Task 8 correction is `8166712 test: bound memory
acceptance and compare complete usage`. The ignored test was not run locally.

**Decision: approved for the coordinator's hosted-run gate. Both prior findings
are resolved, with no new blocker found.**

### P1 resolved: internal work now expires before the CI backstop

`tests/claude_reporting_load.rs:838-840` establishes one absolute 600-second
work deadline before preflight and helper startup. Helper creation, both large
source-construction loops, every EOF barrier, and every sequential parse poll
check that same deadline. Phase and per-helper deadlines are capped with
`min(work_deadline, ...)`, so their former sums cannot extend total work beyond
ten minutes.

Normal cleanup retains the existing 50 two-second cancellation budgets, at most
100 seconds in sequence. That puts the source-level bound at 700 seconds and
leaves 200 seconds before the unchanged 900-second workflow step timeout. A work
deadline failure unwinds the owned controllers and temporary directory before
the shell records `PIPESTATUS`; the outer timeout remains a separate backstop for
an unexpected blocking OS/filesystem operation. The hosted report states this
limit accurately and does not turn it into a real-time scheduling guarantee.

### P2 resolved: complete nonzero usage is retained and compared

Every indexed record now contributes canonical input 6, output 5, cache-read 2,
cache-write 1, and reasoning output 4. At 65,536 identities the asserted totals
are therefore 393,216, 327,680, 131,072, 65,536, and 262,144 respectively.
`memory_eof_barrier` checks the complete `UsageTotals`, including Conversation
scope and Partial coverage, and returns the per-helper raw-phase values.

After newline-triggered parsing, each helper must report `accounting_limit`, the
same 65,536 identities, the same exact 16 MiB charge, and full `UsageTotals`
equality with its own pre-parse snapshot. This closes the earlier partial
input/output assertion and meaningfully exercises every optional token category.

### Re-review verification

- Both exact revisions resolve; `git log 8752905..8166712 --oneline` contains
  `dd08be0` and `8166712` in that order.
- The targeted correction diff changes only the memory fixture and its hosted
  report. The workflow's existing 15-minute step and exit-status/artifact capture
  are unchanged and now have the required margin.
- `git diff --check 8752905...8166712` passes.
- The existing 3 GiB `fifty_session_reporting_capacity` body remains unchanged.
- The worker-recorded compile and Clippy passes were inspected; the coordinator
  separately reported a current compile pass. No behavioral high-water result is
  claimed before the hosted run.

**Updated summary:** Standards: 0 open findings. Spec: 0 open findings. Exact
`8166712` is source-approved for hosted execution; the high-water behavior and
cleanup remain unverified until that exact hosted job passes and its artifacts
are reviewed.

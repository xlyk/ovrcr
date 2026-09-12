# Issue #62: one retained local lifecycle failure

Bounded diagnosis of one retained local failure from the issue #58 installation
work. The case, original revision and retained evidence were named before any
reproduction ran. Each attempt in `attempts/` keeps its command, revision, host
load, exit status and complete output. Prior failures stay recorded as failures
in their own records; nothing here rewrites them.

Result: reproduced on 2026-09-12 with the cause attached. It is host contention
crossing the product's fixed one-second version-probe budget, followed by the
designed fallback. No product defect was found and no production code, deadline
or assertion changed. The merged fixture serialization from PR #67 reduces the
exposure without bounding it; its coverage cost is stated below.

## Selected case

- Test: `agent_admission_failed_unavailable_publication_disconnects_watch` in
  `tests/server_lifecycle.rs`, one of five cases sharing
  `assert_initial_admission`. It failed in every retained full or lifecycle-only
  run before the fixture serialization landed. Three sibling names
  (`..._lost_bind_then_clear_resolves_without_another_startup`,
  `..._failed_bind_status_disconnects_watch`,
  `..._branch_freezes_without_replacement_or_reopening`) failed intermittently
  at the same fixture line.
- Symptom: `supervisor must select a fresh UUID`, probe file length 12 rather
  than 36. The fixture terminal showed `agent admission unavailable; running
  native command`, native argv unchanged, `tty=true`, and the fixture version
  script's first marker file absent (`lifecycle-diagnostic-02.log`).
- Original revisions: `86ef46b8eff3128566f76022c2cfef49714031ac`
  (`research/issue-58-ready-install/attempts/05-full.*`, 2 failed) and
  `b2fc7d8aa3198881340457b40f773df85dcb8d3f`
  (`research/issue-58-ready-install/completion/full-attempt-01.*`, 3 failed;
  `lifecycle-diagnostic-01.*`, 2 failed; `lifecycle-diagnostic-02.*`, 3 failed).
- Retained mitigation: `a42cb12` (merged in PR #67) reused the fixture
  `env_lock()` to serialize the five cases. Its record states the low-level
  cause (spawn failure, timeout or another early failure) was not proven.

## Reproduction design

- Worktree `.worktrees/issue-62-lifecycle-diagnosis`, branch
  `claude/issue-62-lifecycle-diagnosis`, from main
  `b7f139f62b66eb10f73f22ffff51b8b074163fc3`. Own `target/`, own fixture
  temporary directories, sockets and configuration through the existing
  `ControlFixture` isolation. The installed live server was not used and no
  native provider was launched.
- Host: macOS, Darwin 25.5.0, 14 CPUs, `/bin/sh` re-executing `/bin/bash`
  (`/var/select/sh -> /bin/bash`). Installed `claude --version` (2.1.269)
  completed in 0.00–0.01 s in three timed runs, for scale.
- Diagnostic-only patch (`attempts/diagnostic-only.patch`, never committed and
  reverted before delivery): removes the fixture `env_lock()` guard to restore
  the original five-way concurrency, and instruments `src/cli/agent.rs` entry,
  `receiver` entry/decision and `probe_command` in `src/report/admission.rs` to
  record timestamps, spawn latency and the exact reason a probe returns nothing
  (spawn error, `fcntl` failure, read error, oversized output, `waitid` error,
  child exit status, or deadline exceeded). The fixture's `ADMISSION_READY`
  wait keeps its two-second deadline and reports a timeline instead of a bare
  panic. Attempt 01 ran an earlier subset of this patch without timestamps; the
  retained file is the final superset. Attempt 06 ran the same instrumentation
  with the merged guard restored.
- Budget: three default-threaded runs of the lifecycle binary
  (`cargo test -p ovrcr --test server_lifecycle --all-features --locked
  --offline`), which reproduced the failure in the retained diagnostics without
  the full workspace, plus focused isolated, five-case and guard-restored runs.
  Host load (1/5/15-minute averages) is recorded before each run. A sibling
  worktree (`issue-61-unread`) ran its own lifecycle suite on this host during
  part of the session; it was left untouched and counts as host load here.

## Attempts

Times are relative to the fixture session creation. "Entry" is when the fresh
`ovrcr` executable reached `agent run`; "probe" is the bounded `--version`
child's lifetime from spawn to exit or kill.

| Attempt | Fixture guard | Scope, threads | Load before | Outcome |
| --- | --- | --- | --- | --- |
| [01](attempts/01-lifecycle-default.json) | removed | lifecycle binary, default | 11.43 (immediately after the build) | Failed 5: every admission case missed the 2 s `ADMISSION_READY` window with an empty terminal. |
| [02](attempts/02-isolated-baseline.json) | removed | selected case alone | 7.27 | Passed. Entry +545 ms; probe exited 0 after 171 ms. |
| [03](attempts/03-five-admission-cases.json) | removed | `agent_admission_` (8 tests), default | 5.70 | Passed 8. Five-way admission concurrency alone did not reproduce. Timelines were captured by libtest because `--nocapture` followed a second `--`. |
| [04](attempts/04-lifecycle-default-timeline.json) | removed | lifecycle binary, default | 5.09 | Passed 81. Entry +548…+588 ms; probes 219, 647, 750, 858 and 972 ms, all exit 0, spawn 0–1 ms. |
| [05](attempts/05-lifecycle-default-timeline.json) | removed | lifecycle binary, default | 6.49 | Failed 2. Probes 477, 582 and 885 ms passed; two hit `deadline exceeded after 1005 ms`/`1003 ms (spawn 0 ms) bytes=0`. One produced the exact retained signature `supervisor must select a fresh UUID; got 12 bytes` with the terminal showing the fallback; the other, with entry at +694 ms, also missed the 2 s window. |
| [06](attempts/06-lifecycle-default-with-env-lock.json) | restored (main) | lifecycle binary, default | 4.69 | Passed 81. First case: entry +771 ms, probe 479 ms. Remaining four ran near the end of the suite: entry +12…+37 ms, probes 125–151 ms. |

## Classification

- Product defect: none found. The probe child spawned in 0–1 ms in every
  timeline; no spawn, `fcntl`, read or `waitid` error occurred. The only
  failing path was the designed one-second deadline, after which admission
  correctly reported `agent admission unavailable; running native command`,
  passed the native argv unchanged and still reached `ADMISSION_READY`.
- Deadline: the product budget is one second of wall time from spawn. Real
  `claude --version` needs about 10 ms on this host, so the budget is generous
  for the product's target and tight only for a shell script on a saturated
  host. It was not changed.
- Contention: the full lifecycle binary at default test threads stretches the
  22-byte `/bin/sh` probe from 125–171 ms to 219–1005 ms. The five admission
  cases alone (attempt 03) passed. Hosted Ubuntu CI uses dash for `/bin/sh`
  and starts it far faster than this host's bash re-exec, which is one reason
  hosted runs do not show this local behavior.
- Fixture cold startup: the fresh debug `ovrcr` executable reached `agent run`
  0.55–0.77 s after its shell started under contention, 12–37 ms when warm and
  uncontended. This precedes the probe budget, so it does not cause the UUID
  failure; it consumes the fixture's two-second `ADMISSION_READY` window and
  explains attempt 01's empty-terminal signature at load 11.
- Ownership cleanup: after each run no process from this worktree's binaries
  remained and no `.tmp*` fixture directory remained in the temporary root.
  The sibling worktree's test processes were not signaled.

## Coverage of the merged serialization

`a42cb12` holds `env_lock()` for the five admission cases.

- No longer exercised locally: five concurrent fresh-executable admissions
  against five servers, and any overlap between an admission case and the
  other tests holding the same lock.
- Still provided: each case's internal proxy fault, lost-bind, rejection,
  clear/branch and cleanup checks; concurrency with the lifecycle tests that do
  not take the lock.
- Not provided: a bound on the probe's exposure to host load. Attempt 06's
  first serialized case still ran under contention with about 2x margin. The
  mitigation lowers recurrence probability; it does not make the case
  deterministic. A recurrence under the guard should be read as this same
  contention class unless its probe diagnostic says otherwise.

## Results by kind

- Local automated (macOS): the attempts above, at
  `b7f139f62b66eb10f73f22ffff51b8b074163fc3` plus the diagnostic-only patch.
  Delivered changes are documentation and this record only; per the testing
  guide, no Rust suite was rerun for prose.
- Native provider: none run. No Claude or Codex process was launched.
- Linux and hosted full suite: not run locally. Current-head CI on the pull
  request is the hosted evidence and is reported separately there.

## Remaining gaps

- The retained failed runs stay failed. This record does not claim a fix.
- If the case recurs under the guard, the next bounded step is a fixture-only
  decision with a stated coverage change, not a production deadline change.

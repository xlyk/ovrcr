# Task 2 worker report

Baseline: `79d56a4`, branch `codex/codex-reporting`, worktree `/Users/xlyk/Code/ovrcr/.worktrees/codex-reporting`. No staging, commits, pushes, live provider config, credentials, or native GUI actions performed by worker. Coordinator owns native acceptance and diary.

## Files

- `crates/ovrcr-runtime/src/agent_runner.rs`: runtime-computed native-root request metadata, OS peer UID/PID and parent checks on macOS/Linux, unreaped native lifetime anchor, publish anchor before starting handler, clear before reap.
- `src/report.rs`: provider-neutral reservation and payload helper; Claude wrappers preserved.
- `src/report/admission.rs`: share bounded version-probe mechanics; Claude ignores new request metadata.
- `src/report/codex.rs`: pinned fresh-launch gate, authenticated hook state, duplicate/overlap/identity limits, receipt-based Bind/rebind, no collector.
- `src/cli/{agent,args,report}.rs`: positional provider plus legacy `--provider`, Codex managed adapter and silent stdin command, reporting failure native fallback for Codex.
- `tests/server_lifecycle.rs`: four actual CLI/PTY/socket regressions and one ignored native child helper.
- `tests/codex_reporting.rs`: unmanaged silence, grammar, native argv/output/exit.

## Retained attempts

Every Cargo command below had prefix `rtk proxy env CARGO_INCREMENTAL=0` and ran from this worktree. `--nocapture` was supplied. Socket/PTY runs after attempt 1 used scoped elevated execution, approved by automatic review. No failed attempts are counted as passed.

1. `cargo test -p ovrcr --test server_lifecycle codex_managed_hooks_ready_interrupt_duplicates_and_rebind -- --exact --nocapture`: sandbox exit101, 0 passed /1 failed /80 filtered. Isolated server socket bind returned `Operation not permitted`; no behavioral RED. Fixture preserved `/var/folders/j9/h6rlffxd1r794s9g9ps_52t40000gn/T/.tmp0WXSXc`.
2. Same exact command elevated: exit101, 0 passed /1 failed /80 filtered. Behavioral RED: terminal `error: unexpected argument 'codex' found`, native marker absent, `CODEX_NATIVE_EXIT=2`.
3. Same exact command after initial implementation: exit0, 1 passed /80 filtered (3.46s).
4. `cargo test -p ovrcr --test server_lifecycle codex_managed -- --nocapture`: exit101 compile failure, no tests executed; incorrect test helper name `wait_process_absent` corrected to existing `wait_pid_absent`.
5. Same exact command: exit0, 3 passed /80 filtered (4.36s).
6. `cargo test -p ovrcr --test codex_reporting -- --nocapture`: exit0, 3 passed /0 filtered (0.91s).
7. `cargo test -p ovrcr --lib report::codex::tests -- --nocapture`: exit0, 3 passed /26 filtered (0.29s).
8. `cargo test -p ovrcr --test server_lifecycle codex_managed -- --nocapture` after native-death/startup-conflict cases: exit101, 2 passed /2 failed /80 filtered. Expected RED: reservation conflict prevented native launch (actual1, required23). Also unexpected missing initial snapshot in overlap test. Fixed Codex reservation errors to preserve native launch; initial admission failure remains under diagnosis.
9. Same exact command after fallback fix and better failure context: exit101, 3 passed /1 failed /80 filtered. Lost-Bind test had no initial snapshot; no Bind request observed.
10. `cargo test -p ovrcr --test server_lifecycle private_claude_activity -- --nocapture`: exit0, 2 passed /82 filtered (3.06s), actual Claude managed activity and failed-delivery isolation.
11. `cargo test -p ovrcr-runtime --lib agent_runner::tests -- --nocapture`: exit0, 2 passed /98 filtered (0.03s), exact65,536 and oversized envelope tests.
12. `cargo test -p ovrcr --test server_lifecycle codex_managed -- --nocapture` with missing-snapshot terminal diagnostic: exit101, 3 passed /1 failed /80 filtered. Terminal emitted `Codex reporting unavailable; running native command` before native marker. This narrows loss to pre-spawn eligibility, not runtime immediate-callback identity initialization.
13. `cargo test -p ovrcr --test server_lifecycle codex_managed_lost_bind_receipt_recovers_before_busy -- --exact --nocapture` with explicit safe startup failure reasons: exit0, 1 passed /83 filtered (2.56s).

## Later attempts and final evidence

Full logs from attempt16 onward are in sibling `task-2-attempts/`. `early-failures-tool-output.log` retains verbatim failure excerpts; complete early tool outputs remain in the task trace. Each later log embeds its exact command, result and exit status. All test runs use `CARGO_INCREMENTAL=0`; no broad workspace suites ran.

14. Original four-way managed filter: exit101, 3 passed /1 failed (native-death fixture initial snapshot absent).
15. Same filter after all-fixture startup assertion: exit0, 4 passed. No code fix, not treated as resolution.
16. Same filter: exit101, 2 passed /2 failed. Both failures explicitly identified `version probe unsupported or unavailable` before native spawn; reservation/terminal checks succeeded.
17–18. Same filter with temporary safe probe diagnostics: each exit0, 4 passed. Not treated as resolution.
19. Isolated exact pinned-version test: exit0, 1 passed /3 filtered. Exact output18bytes/status0; first child201ms, later children6ms.
20. Managed filter with diagnostic: exit0, 4 passed, not treated as resolution.
21. Four barrier-synchronized independent immediate shell probes: exit0, 1 test passed. Exit0/output18bytes at209,342,470,606ms.
22. Eight-probe diagnostic: exit101, 1 test failed. First seven completed151,285,419,554,684,846,949ms; eighth explicitly hit deadline at1.001921125s with0bytes, was killed and reaped with signal9. This demonstrates host startup can exhaust the unchanged one-second policy; it does not individually prove every earlier four-way failure's cause. Diagnostic stress test and temporary syscall logs removed from code; evidence retained.
23. Approved serialized managed filter: exit0, 4 passed /80 filtered.
24. Dedicated CLI/probe suite: exit101, 4 passed /1 failed. New blocked-script probe never wrote its PID marker before deadline; test did not reach its intended FIFO block. Replaced with a deterministic regression beside the shared probe implementation, launching existing `/bin/sh -c` through a minimal internal Command-taking primitive. No deadline/lifecycle algorithm change.
25. Self-review SessionEnd regression: exit101, 0 passed /1 failed /83 filtered. SessionEnd after Stop failed to close reporting; following prompt incorrectly becameBusy. Corrected current accepted turn SessionEnd handling.
26. Final managed CLI/PTY/socket suite: exit0, 4 passed /80 filtered (5.97s). Includes SessionEnd, all activity/correlation cases, wrong-parent/child filtering, A→B→A, stale-generation rejection, receipt recovery, missing-ending overlap, native death, stdout/exit/PID/socket cleanup.
27. Final dedicated CLI suite: exit0, 4 passed /0 filtered (0.80s).
28. Final receiver capacity/duplicate unit suite: exit0, 3 passed /27 filtered (0.22s).
29. Final unchanged Claude managed activity regressions: exit0, 2 passed /82 filtered (2.02s).
30. Shared probe unit regressions: exit0, 2 passed /28 filtered (1.11s). Direct-shell FIFO block reaches timeout then exact process group is absent; existing Claude pinned probe also cleans descendants after leader exit.

31. Existing native launcher signal/terminal ownership regression: exit0, 1 passed /83 filtered (2.11s); native PGID owned, terminal restored after forwarded TERM, private endpoint removed.
32. `cargo check -p ovrcr --all-targets`: exit0 (13.08s), no warnings.
33. `cargo fmt --all -- --check`: exit0.
34. `git diff --check`: exit0.

Code is ready for independent review. This is not completed Task2 acceptance: pinned native two-turn/Interrupt parent-ordering verification remains required.

## Self-review

- Trusted root metadata is computed only from kernel peer UID/PID, live OS parent, and this supervisor's unreaped native child. Receiver startup waits until native identity is published. Anchor lock is not held over provider callbacks or transport. It is cleared before reaping, and waitid rejects an already-waitable exit.
- Codex is separate from Claude interpretation. No collector, transcript read, serialized PPID trust, text inference, timeout completion, or old private/legacy fallback added.
- Duplicate identity check precedes revisions, Bind and freshness. Seen identities remain bounded without eviction; overlap disables. Event-specific turn matching prevents old/mismatched Stop or Interrupt from closing the active turn.
- Bind uses expected current binding; receipt loss resolves original operation before Busy publication in the returned generation. Delivery uncertainty closes the watch. Native completion shuts down lease transport before Drop, avoiding an extra blocking grace period.
- Claude compatibility entrypoints, parser behavior and native runtime signals remain. Codex reservation errors preserve native execution as required; existing Claude conflict semantics remain.
- CLI help now describes pinned Codex grammar and known missing-hook/API-failure limitations. Task3 setup/doctor remains outside scope.

## Cleanup

The sole failed pre-start sandbox fixture `.tmp0WXSXc` had no socket and had never created a PTY/session. Its file inventory and ownership basis are retained in `task-2-attempts/cleanup.json`; the task-owned directory was removed. All successfully started test fixtures ran their existing scoped Drop cleanup (`server stopped` in logs). Native death and normal completion tests additionally assert exact native PID absence and private endpoint removal. No live/user-owned resources or old acceptance harnesses were touched.

## Native acceptance instructions for coordinator

After exact-revision review/build, create a task-owned provider home and workspace. Use existing credential authorization only through coordinator; worker did not access credentials. Preserve separate OVRCR_CONFIG/OVRCR_SOCKET and CODEX_HOME. Do not modify live Codex config.

Generate this stanza for each `SessionStart`, `UserPromptSubmit`, `Stop`, `Interrupt`, `SessionEnd` (substitute EVENT in both table names):

```toml
[[hooks.EVENT]]
[[hooks.EVENT.hooks]]
type = "command"
command = "exec '/Users/xlyk/Code/ovrcr/.worktrees/codex-reporting/target/debug/ovrcr' report codex --stdin"
```

For SessionStart add `matcher = "startup|resume"` before its nested hooks table. Do not set async. Explicit shell `exec` is required: the connected reporter's kernel PID must have the supervised native PID as its immediate OS parent. A wrapper that forks the reporter is rejected.

From an actual managed OVRCR PTY, run:

```text
/Users/xlyk/Code/ovrcr/.worktrees/codex-reporting/target/debug/ovrcr agent run codex -- /opt/homebrew/Caskroom/codex/0.153.0/bin/codex --no-alt-screen -C OWNED_WORKSPACE 'Reply exactly OVRCR_CODEX_ONE.'
```

Supply the second fixed harmless prompt with native input, then a separate long-running harmless prompt interrupted by native Ctrl-C. Observe Busy→Ready per completed root turn, Busy→Idle on Interrupt, no later Ready from that interrupted turn. Capture native output and OVRCR inventory at transitions. Do not answer approvals automatically. Verify direct-exec relationship and synchronous ordering under pinned native implementation; synthetic tests do not certify it. Stop this unit if the native parent or ordering proof fails; do not relax to claimed PPID or transcript paths.

## Review status and gaps

- Coordinator approved serializing these lifecycle/correlation fixtures through existing `env_lock`, retaining all behavioral assertions and the one-second production probe deadline. Host executable startup can exhaust that deadline (demonstrated below); the precise syscall result of each earlier four-way failure was not captured, so those failed attempts remain failures and are not claimed fixed/passed. This is not a launch-throughput guarantee.
- Native two-turn/interruption proof and Linux execution remain unrun.
- Identity count/charged-storage exhaustion currently exercised as real receiver unit boundaries; not 65,536 native subprocess callbacks.
- No broad workspace suite, clippy, or CUA claim; coordinator assembles required acceptance later.
- API errors without native Stop can leave Busy; missing ending hooks disable on overlap. No disappearance-based completion.

## Worker handoff content hashes

```text
8042616ccd0246abedb0916fd6ee257815744d7e0eacb127a2037d33f12346d4  crates/ovrcr-runtime/src/agent_runner.rs
60c60b176817af5eab9479415073488ebb4639caf9c55b68bfaebd1bdf524c39  src/report.rs
381f7992c88542691bf6647df08e37734915b4f5780c7ccaf20ff5fa7f2cedd2  src/report/admission.rs
88d3c33d42a67c79f63882f1360a0c7b482ac945909f22c10a75a011e8b7867e  src/report/codex.rs
fbc8005d81b80b18c74b0369fa4fa6916b4d64254496b2c00bb73fc8470581cd  src/cli/agent.rs
a930130dc467447d6bc977e8d7ec42e112fd4fbeb809627d207afb4555520e7a  src/cli/args.rs
63b5b3b28b882875d134502b1cf9035244b0e7eac2b01577dd94ca16c9f55297  src/cli/report.rs
a830df19e3385620ed6552f1bee74b2921e9bf19f96fabfce61153e94a36de44  tests/server_lifecycle.rs
e97b2abc9a34a455d9916f60cd7860d78fff7e1c323ae5c3a3bb3003d8f85f54  tests/codex_reporting.rs
```

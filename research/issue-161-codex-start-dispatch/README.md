# Issue #161: Codex SessionStart dispatch coverage

This bounded change repairs generated hook coverage. It does not complete
[#161](https://github.com/xlyk/ovrcr/issues/161), support a fork replacement,
or establish genuine provider/notification acceptance. It is stacked on
[#243](https://github.com/xlyk/ovrcr/pull/243) at
`7b94e602ba0b9c5b1ad4fb67f4d8ec34effe71d9`; the earlier tested heads remain unchanged.

## Defect and resulting behavior

Setup previously printed `matcher = "startup|resume|clear|compact"` for its
SessionStart reporter, and doctor/boot recognized that filter as sufficient
coverage. Codex 0.155.1 emits a Root `fork` source. The filter suppresses that
callback before OVRCR's existing unsupported-identity guard can see it, leaving
reporting connected and a prior approval request open.

Setup now prints an unfiltered exact synchronous reporter. Only an absent or
empty matcher satisfies coverage. Composition preserves existing filtered
handlers, their order and opaque trust/permission values, then appends one
unfiltered group. Repeating composition is idempotent. Setup still prints only;
the operator must review the composition and trust the added handler natively.
Overlapping legacy/new groups may both run for supported sources. No trust,
permission, global configuration or native process changes are made by setup.

The receiver remains unchanged. An unsupported source while bound freezes health
with `identity_transition_unavailable`, closes pending approvals, retains the
binding and last historical activity, and preserves Unread. Late callbacks
cannot revive that reporting lifetime. The native terminal remains usable.
Configuration repair does not repair an already frozen lifetime.

## Pinned primary source

The installed Codex executable resolves to the Homebrew 0.155.1 artifact. Source
was retrieved read-only from annotated tag `rust-v0.155.1`, resolving to
`be2951ea34f0d295ed0becf97079f92fa5f6950e`. The
[native source manifest](native-source-manifest.json) records paths, exact revision,
SHA-256 digests and successful retrieval commands.

- [Session creation](https://github.com/openai/codex/blob/be2951ea34f0d295ed0becf97079f92fa5f6950e/codex-rs/core/src/session/session.rs#L1692)
  queues `Fork` when forked history has a fork parent; resume without a fork parent
  remains `Resume`.
- [Hook runtime](https://github.com/openai/codex/blob/be2951ea34f0d295ed0becf97079f92fa5f6950e/codex-rs/core/src/hook_runtime.rs#L125)
  drains pending sources with a turn context. Spawned child agents use
  SubagentStart; Root events use SessionStart.
- [SessionStart](https://github.com/openai/codex/blob/be2951ea34f0d295ed0becf97079f92fa5f6950e/codex-rs/hooks/src/events/session_start.rs#L74)
  supplies the source string to both preview and execution selection.
- [Dispatcher](https://github.com/openai/codex/blob/be2951ea34f0d295ed0becf97079f92fa5f6950e/codex-rs/hooks/src/engine/dispatcher.rs#L41)
  matches SessionStart handlers against that input.
- [Matcher](https://github.com/openai/codex/blob/be2951ea34f0d295ed0becf97079f92fa5f6950e/codex-rs/hooks/src/events/common.rs#L137)
  accepts absent/empty matchers for all inputs; the old exact alternation excludes
  fork. Native command hooks may execute concurrently. The controlled fixture
  does not certify native concurrency, trust or effective configuration.

Unfiltered coverage cannot observe a transition before Codex emits its hook.
The turn call sites and queued dispatch preserve the silent clear/history/fork
window identified by the [prior source review](../issue-161-codex-doctor/README.md).
The command hook event set still provides no distinct genuine question open/close
surface. No new source authority or readiness semantics were added.

## Regressions and evidence

The unit recognition case and real doctor CLI case reject filtered-only
SessionStart coverage while leaving trust/delivery unverified.
`codex_setup_dispatches_fork_to_existing_identity_guard` obtains TOML from the
actual setup CLI, loads it in an owned synthetic Codex child and executes the
actual emitted shell command. It uses the real managed launcher, private native
parent attribution, Reporter and isolated server/PTY paths. Its bounded matcher
model covers only generated unfiltered groups and the legacy exact alternation;
it is not a native provider simulation for arbitrary regexes or hook trust.

For fresh and legacy composition it asserts:

- Existing filtered group and supplied file bytes, trust state and permission
  policy survive composition; one unfiltered group is appended, then repeated
  composition is identical.
- Startup overlap does not invent a binding generation. Root prompt/Stop creates
  Unread; a later attributable PermissionRequest creates one pending approval.
- Fork dispatch reaches one reporter, freezes the same binding, clears that wait,
  preserves historical activity/Unread and keeps the terminal Running.
- Prior-Root and forked-Root late Stop/prompt callbacks leave that exact frozen
  snapshot unchanged. Sending exit through the real server reaches the child,
  which retains its native exit status 17.

The original failing runs are retained separately from later passes. The first
post-fix run failed a newly added, incorrect expectation that unavailable health
would erase the last activity. Existing Reporter/server semantics and setup docs
explicitly retain historical activity. That assertion was corrected to exact
preservation; health, request cleanup, binding, Unread and native usability
assertions remain in force. No product assertion or existing gate was weakened.

The [normalized check logs](checks/) and [hash manifest](evidence-normalization.json)
retain original attempts; private task evidence keeps the original bytes. Metadata
records the tested base plus working-diff SHA. The [source/test snapshot](checks/66-start-dispatch-source-snapshot.json)
records the final three code/test file hashes for comparison to the committed
content. Independent review found no blocking/high/medium findings.

| Check | Result |
| --- | --- |
| 59 original recognition regression | Failed, 1 test: legacy filtered coverage incorrectly accepted. |
| 60 original generated-command managed regression | Failed, 1 test: health stayed Connected after fork. |
| 61 original real doctor regression | Failed, 1 test: legacy filtered-only file incorrectly certified. |
| 62 first post-fix managed attempt | Failed, 1 test: new activity-erasure expectation contradicted retained-history contract; preserved and corrected to exact historical observation. |
| 63 managed fresh + legacy regression | Passed, 1 test exercising both compositions. |
| 64 setup/doctor integration suite | Passed, 13 tests. Final strengthened assertions also passed in 66. |
| 65 affected Codex callers | Passed, 13 tests; 1 intentional native-child-helper ignore; 113 unrelated cases filtered. |
| 66 full serial workspace/all-targets/all-features | Passed, 1,254 tests; 0 failed, 21 ignored, 0 filtered across 34 executables. |
| 67 Node invocation at assumed path | Not executed: path absent, runner exited 1; no tests ran. |
| 68 verified installed Node 22.23.2 | Passed, 47 extension tests. |
| 69 all-target/all-feature Clippy | Passed with `-D warnings`. |
| 70 formatting | Passed. |
| 71 workspace doctests | Completed successfully, 0 doctests. |
| 72 cleanup + hash check | No matching test-binary-prefix processes or cleanup-incomplete logs; final source/test hashes unchanged. |

No source or test edits followed the completed workspace snapshot. Current-head
hosted check results belong in the draft PR; automatic Linux skips are not Linux
acceptance. Full native acceptance remains open as listed below.

## Remaining issue acceptance

Genuine Root questions and independently correlated closing boundaries remain
unverified, including concurrent questions, duplicates, cancellation and
automatic-answer exclusion. The installed Claude 2.1.286 artifact was read without
launching it (SHA-256 `75e3016e9d2570767b08e43a7467d4817a4f149232c169ca295f2c95fef21433`).
Its Notification construction adds message/title/type to common session/prompt
context, without a question request ID. Elicitation has separate pre-decision
and result hook structures, with optional elicitation IDs. These source facts do
not establish an independently identifiable genuine human dialog open/close;
no mapping was manufactured from tool lifetime or a pre-decision hook.

Continuous foreground identity, Claude silent background-fork boundaries,
Codex fork/backtrack windows and every native clear/history route remain open.
Genuine provider Input/Reopen/switch and native desktop/sound/platform acceptance
remain separate. No genuine provider, account, live session, desktop UI,
notification, sound, credential or permission operation was performed for this
slice. A reporting warning is a fallback, not completion of full parity.

## Cleanup

The shared Live fixture owns its private registry/socket/workspace and tracks
only task-owned groups. Its Drop path shuts down that server, checks owned group
absence, and preserves the temporary root if cleanup is uncertain. Scoped/full
logs contain no cleanup-incomplete report. The read-only process inventory was
limited to this worktree's target binary prefix; it is not a global inventory or
an ownership claim over unrelated live sessions. No GUI, bridge or separate
provider process was started. The isolated worktree and evidence remain for
review, recovery and stacking; the original checkout and prior tested PR heads
were untouched.

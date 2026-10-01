# Issue #161: Codex session diagnostics and remaining source gates

This repairs the existing `agent doctor codex --session ID` command for
[#161](https://github.com/xlyk/ovrcr/issues/161). It is stacked on the reviewed
Claude repair in [#240](https://github.com/xlyk/ovrcr/pull/240), exact base
`d480fb384c1c8b04f8090f353f2944d4ef0a6f1d`. Neither slice completes the parent
issue or accepts permanent product limits.

## Diagnostic behavior

Codex doctor accepted `--session` but always returned
`not_inspected_use_session_usage`. Persistent recovery warnings directed users
to a command that could not diagnose their session.

Doctor now uses the existing read-only `Inspect` path for an explicit session ID
or inherited `OVRCR_SESSION_ID`. An explicit ID wins. It reports public provider
and generation, reporter health and recognized diagnostic codes, and explains
that `identity_transition_unavailable` requires a fresh supervised invocation
when the user is ready. Configuration repair cannot recover that frozen lifetime.
Private invocation/conversation identities and arbitrary reason text remain
redacted. `remediation` remains a string. Provider mismatch, no binding, missing
session and inspection failure have distinct statuses; no request uses
`not_requested`.

Inspection does not start a server, mutate provider settings/trust or revive a
binding. The retained offline session loader supplies no live agent binding;
doctor does not certify delivery from retained history. The selected provider
executable is invoked only for its bounded version probe. No callback mapping,
wire schema, native process restart or provider capability is added.

The affected CLI/setup docs also correct stale claims that all Codex Input is
unavailable and that eligibility is limited to 0.153.x patches. Existing code
already supports verified root approvals and uses the shared stable version
floor; question support and tested-version evidence remain separate.

## Regression and verification

- [Behavioral RED](checks/36-codex-doctor-red.log): the real managed fixture, PTY, private reporter transport, server and CLI returned `not_inspected_use_session_usage` after an identity freeze. The assertion expected `reporting_unavailable` and failed on the original behavior.
- [Initial GREEN](checks/37-codex-doctor-green.log): the same application path reported the exact identity reason and retained binding. A later callback could not revive reporting; subsequent terminal input reached the still-running native fixture.
- [Initial privacy suite](checks/38-doctor-setup.log): all 13 setup/doctor tests passed, including inherited Codex session inspection, arbitrary reason redaction and no-server failure paths.
- [Safe-code unit](checks/39-safe-reasons-unit.log): the existing recognized-code/privacy regression passed using the shared allowlist.
- [Expanded fixture failure](checks/40-doctor-final-focused.log): bounded accept left the accepted stream nonblocking on macOS; its protocol read failed with `EAGAIN`. The fixture now explicitly restores blocking I/O after bounded accept. No product assertion or deadline was weakened.
- [Final privacy suite](checks/41-doctor-final-focused.log): 13 passed, zero failures/ignores/filtered tests. It also covers a non-Codex binding, suppression of that provider's health and explicit `--session` precedence over inherited ID.
- [Complete serial workspace](checks/42-workspace-serial.log): **1,253 passed, zero failures, 21 ignored, no filters**, across 34 test executables with all targets/features. The new managed regression and the existing admission-sensitive retained recovery cases passed. The existing hosted-excluded `partial_resize_connection_delivers_error_before_owner_close` also passed locally.
- [Clippy](checks/43-clippy.log), [formatting](checks/44-format.log), and [Node 22 extensions](checks/45-extensions.log): passed; 47 extension tests executed, none skipped.
- [Doctest command](checks/46-doctests.log): completed successfully with zero tests; this supplies no behavioral acceptance.

The [source/test snapshot](checks/42-source-test-snapshot.json) matches all four
final source/test files. Later tracked edits only correct the affected CLI/setup
prose; no tested code changed. Final independent review is recorded before
publication. Each check retains command, source revision, diff hash, timestamps
and exit status in its sibling JSON. The logs remove only trailing ASCII padding
and extra empty EOF lines; [raw/normalized hashes](evidence-normalization.json)
identify both byte sets. Original attempts remain in the task evidence directory.
The original checkout and #240's exact tested branch/HEAD remain unchanged.

## Pinned source review, 2026-09-30

The local read-only version probe reported Codex CLI **0.155.1**. Public tag
`rust-v0.155.1` resolves to
`be2951ea34f0d295ed0becf97079f92fa5f6950e`. The
[source manifest](source-manifest.json) records reviewed public files, hashes and
successful read commands. This is source inspection, not new native acceptance.

The [session constructor](https://github.com/openai/codex/blob/be2951ea34f0d295ed0becf97079f92fa5f6950e/codex-rs/core/src/session/session.rs#L1692-L1720)
maps new, resumed, cleared and forked histories to separate sources, then queues
SessionStart. The [turn caller](https://github.com/openai/codex/blob/be2951ea34f0d295ed0becf97079f92fa5f6950e/codex-rs/core/src/session/turn.rs#L320)
and [hook runtime](https://github.com/openai/codex/blob/be2951ea34f0d295ed0becf97079f92fa5f6950e/codex-rs/core/src/hook_runtime.rs#L125-L178)
dispatch that pending hook with a turn context. A distinct `fork` source now
exists in the [source enum](https://github.com/openai/codex/blob/be2951ea34f0d295ed0becf97079f92fa5f6950e/codex-rs/hooks/src/events/session_start.rs#L24-L40);
its existence does not establish continuous foreground identity before the next
turn. The older retained 0.153.0 startup/backtrack evidence remains valid for that
version; this review does not present it as every later version's source grammar.
The generated OVRCR SessionStart matcher still selects
`startup|resume|clear|compact`; this slice does not widen that filter or claim
native fork dispatch. The existing bound handler freezes an unexpected source
if it is received. A later fork callback still cannot establish identity during
the preceding silent interval.

Both [clear UI routes](https://github.com/openai/codex/blob/be2951ea34f0d295ed0becf97079f92fa5f6950e/codex-rs/tui/src/app/event_dispatch.rs#L293-L324)
request a fresh session with `ThreadStartSource::Clear`. The
[replacement helper](https://github.com/openai/codex/blob/be2951ea34f0d295ed0becf97079f92fa5f6950e/codex-rs/tui/src/app/session_lifecycle.rs#L898-L955)
starts the replacement before calling the
[UI shutdown helper](https://github.com/openai/codex/blob/be2951ea34f0d295ed0becf97079f92fa5f6950e/codex-rs/tui/src/app/thread_routing.rs#L40-L49),
which unsubscribes the old root's events and aborts its listener. That helper
does not itself shut down the root core session. These inspected routes and lazy
hook timing do not prove immediate invalidation or all clear paths natively.

The [command hook event modules](https://github.com/openai/codex/blob/be2951ea34f0d295ed0becf97079f92fa5f6950e/codex-rs/hooks/src/events/mod.rs)
have no distinct question event. The
[request-user-input handler](https://github.com/openai/codex/blob/be2951ea34f0d295ed0becf97079f92fa5f6950e/codex-rs/core/src/tools/handlers/request_user_input.rs#L69-L98)
rejects non-root use, validates arguments and awaits a call-ID-specific request;
its blocking flag depends on collaboration mode. This core request path is not
a verified external command-hook human-wait open/close contract. Generic tool
lifetime must not substitute for question Input.

Claude's [documented hooks](https://code.claude.com/docs/en/hooks) can supply
AskUserQuestion answers through `updatedInput` or resolve MCP elicitation
programmatically; elicitation identity is optional. Those hooks alone do not
prove an independently identified human wait. No question mapping was added.

## Remaining acceptance and cleanup

The full #161 gates remain open:

- Distinct genuine root question open/close identities for Claude AskUserQuestion/MCP elicitation and Codex questions, including concurrent requests, cancellation and deduplication.
- Claude background forks without a leave signal and Codex fork/backtrack or conflicting startup gaps with continuous foreground identity.
- Native evidence for every Codex clear/history route and its callback timing.
- Genuine provider macOS Input/Reopen/switch and desktop notification/sound acceptance.

This CLI repair needs no desktop slot. Tests use owned synthetic fixtures and
isolated sockets, configurations, workspaces and process groups with fixture
cleanup. No live sessions, provider conversations/accounts, OS notifications,
permission changes or production settings were operated. Native provider and OS
acceptance remain unrun, and unavailable platform acceptance is not passed.
The [post-suite process check](checks/47-doctor-cleanup.json) found no executable
from this worktree's target directory remaining; it does not inventory or control
unrelated live sessions. No GUI resources were created in this slice.
The isolated worktree and evidence are retained for PR review; merge and issue
closure require separate authorization.

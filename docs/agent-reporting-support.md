# Agent reporting support record

Claude contract review date: 2026-09-10. Retained discovery and native evidence verify exact **2.1.267** and **2.1.268**. These are tested releases, separate from the current [patch compatibility policy](agent-versions.md). This record describes reporting research for the [Claude implementation plan](../plans/2026-09-09-claude-code-reporting.md); it does not declare the planned integration shipped.

The [remaining-requirement matrix](../research/claude-reporting-acceptance/remaining-matrix.md) reconciles this rolling record with the delivered callers, assertions, tested revisions and native evidence. It splits passing parts from open provider and platform clauses.

The [fixture manifest](../tests/fixtures/agent-reporting/claude/manifest.json) contains 23 **source-derived** payload examples. Every identity, path and content value is synthetic. Shared IDs illustrate relationships; file order does not represent observed callback order. The rolling documentation may describe behavior that differs from the installed release. No fixture in this directory is a live capture.

## Documented sources

| Surface | Fields and interpretation |
| --- | --- |
| Hook identity | `session_id`, optional `prompt_id`, and `transcript_path`; `agent_id` identifies a child. `agent_type` can also name a root session. |
| Session transitions | `SessionStart.source` includes startup, resume, clear, compact and fork. Fork covers both foreground branch and background fork operations. |
| Completion observations | Stop can cause continuation. Interrupted turns do not emit Stop. StopFailure covers API failure. Transcript writes can lag callbacks. |

These fields come from the [official hooks reference](https://code.claude.com/docs/en/hooks), refreshed on the review date. Prompt identity offers correlation; this review does not certify callback order or a final settled notification.

| Measurement | Source and scope |
| --- | --- |
| Current context | `context_window.current_usage`; occupancy uses fresh input plus cache writes and reads, excluding output. Capacity comes from `context_window_size`. Null usage is unknown. |
| Context totals | `total_input_tokens` and `total_output_tokens` describe current context, not cumulative conversation consumption. |
| Cost | `cost.total_cost_usd` is a client-side estimate for the session; clear resets it. Actual billing may differ. |

The [official status-line reference](https://code.claude.com/docs/en/statusline) supplies these semantics. The fixtures use small synthetic values: 30 fresh input + 10 cache-write input + 40 cache-read input = 80 occupied tokens. The pre-response null fixture does not establish post-compaction timing.

## Foreground admission support

| Transition | Task 1 acceptance still needed |
| --- | --- |
| Initial start | Certified for the narrow interactive Claude Code 2.1.267 launch grammar, supervisor-selected UUID and root startup discriminator; actual managed routing captured in attempt09. |
| Foreground branch | Distinguish the new foreground conversation from a background fork. |
| Background fork | Prove the current foreground binding remains unchanged. |
| Clear | Capture identity replacement and callbacks arriving after the next transition. |
| Resume | Capture foreground replacement and exclusion of stale callbacks. |
| Compaction | Prove same-conversation identity persists and capture context replacement. |

The approved design requires identity to become unavailable for an ambiguous candidate. New metrics for that candidate must stop, and a new managed invocation is required. In-process switching is not certified. These restrictions must also appear in setup and doctor output when those commands are implemented; this research record does not implement them.

## Blocking dependencies

| Dependency | Evidence needed before certification |
| --- | --- |
| `live-payloads` | Isolated interactive start/resume, prompts, actual approval wait/allow/deny, tool success/failure, Stop continuation, cancellation, API failure, compaction, session end, children and new/resume/fork captures. |
| `settled-transition` | A supported or verified event after every Stop decision, correlated with the current work. Empty background arrays, assistant text, silence and callback completion do not discharge this gate. |
| `cumulative-accounting` | A supported aggregate or a versioned transcript reader with certified record identity, last-record replacement, replay, resumed/forked history and child/auxiliary coverage. |
| `final-source-completion` | A reliable final-accounting boundary, including delayed writes and abrupt native exit. |
| `cost-scope` | Comparison with native totals establishing child, auxiliary, resumed and forked attribution independently of token coverage. |

Live captures establish equal-usage duplicate rows with the same `(message.id, requestId)` and different record UUIDs. They do not establish differing-value replacement. The partial reader must stop before applying a conflicting counted value and label its retained result unavailable; it cannot claim the retained number remains current or is a lower bound. Recognized root-transcript records do not cover every native usage category. Missing cost remains unknown; explicit zero is a value.

Task 1 remains open. The source fixtures support later parser work only after its prerequisite contracts are resolved. Existing manual reporting instructions remain in [agent-reporting.md](agent-reporting.md).

## Local execution evidence

The [acceptance checkpoint](../research/claude-reporting-acceptance/checkpoint.md) records the executable-version check, three passing existing Claude tests, sandbox-network failure, and isolated native onboarding attempt. Network permission resolved connectivity; the disposable configuration then required authentication. No authenticated turn or live hook payload was captured. Owned fixture PIDs 98073, 98075 and 98076 were terminated and verified absent.

## Authenticated follow-up, 2026-09-10

Authentication was authorized and succeeded after the earlier blocked attempt. [Attempt 05](../research/claude-reporting-acceptance/attempt-05-summary.md) records real startup, consecutive turns, continued Stop, clear, resume, permission wait/allow, tool completion and sanitized usage samples. The source-derived fixture directory remains source-only; live evidence is stored separately. Final settling, complete accounting and transition admission remain uncertified.

## Compaction and implementation gate follow-up

[Attempt 07](../research/claude-reporting-acceptance/attempt-07-summary.md) observed same-conversation compaction and a foreground branch with a new ID. The restricted fixture refused background fork, so that case remains unverified. The approved fallback remains unavailable identity for ambiguous transitions.

Task2 shared runtime work is permitted by the plan's source-or-named-blocker gate. It does not certify a Claude adapter: initial foreground admission, complete final accounting and confirmed settling remain independent provider gates.

## Initial startup evidence follow-up

[Attempt08](../research/claude-reporting-acceptance/attempt-08-summary.md) observed a supervisor-selected UUID on a named root startup, with `agent_type` present and `agent_id` absent. Child lifecycle events carried the same session ID and a distinct `agent_id`, including cases with an empty `agent_type`. Independent review permits implementing this narrow startup discriminator, subject to actual invocation-routing tests and explicit launch-option eligibility. Initial startup remains uncertified until those tests pass; the earlier unsupported-switch limitations remain.

## Reviewed implementation checkpoints

The implementation branch now has independently reviewed shared ownership/protocol, managed launcher, narrow initial admission, observed activity, pure metrics arithmetic and compact dashboard presentation. [Attempt09](../research/claude-reporting-acceptance/attempt-09-managed/commands.md) exercised real managed startup, native output, clear/exit and owned-process cleanup. [Attempt10](../research/claude-reporting-acceptance/attempt-10-sources/summary.md) linked an unanswered approval selector to its correlated permission notification and the exact reported transcript path to matching file records. Earlier research-only statements above describe their original checkpoint, not the current branch.

Activity support requires synchronous hooks. Matching permission_prompt means observed waiting; PermissionRequest alone does not. Stop means observed response end (Ready · observed) when correlated by root `prompt_id`, never confirmed settling; Unread and response-ready alerts follow that same turn identity at most once. Native model-not-found StopFailure now has correlated evidence in [attempt11](../research/claude-reporting-acceptance/attempt-11-api-failure/summary.md); broader API failures remain unverified. Evidence base: [provider-capabilities](../research/claude-reporting-acceptance/provider-capabilities.md), [attempt-05](../research/claude-reporting-acceptance/attempt-05-summary.md) (continued Stop same prompt_id), [native-child](../research/claude-reporting-acceptance/native-child/results.md) (child exclusion). Confirmed settling remains open. Approval Input requests are available from matching root `permission_prompt` notifications with `approval:{prompt_id}` identity and close on the next attributable root activity; AskUserQuestion remains a named blocker (no distinct question surface). Linux TUI/synthetic app-path tests are the merge bar for Ready and approval Input alerts; native macOS CUA is deferred.

## Assembled implementation follow-up, 2026-09-10

Managed context/cost publication, exact-path partial transcript collection, native
finalization, setup/doctor and JSON inspection are implemented. Independent review
accepted the post-exit scan correction `05c3851` and conservative supplied-file
diagnostics `d139563`. Finalization requires a scan requested after native
completion, keeps the original two-second deadline, and always retains Partial
coverage. Neither EOF nor this scan establishes provider accounting completion.

Collector cleanup now retains eventual child-reaping ownership at an expired
deadline (`5585702`) and handles macOS zombie-only process groups by reaping the
owned child and checking group absence without signaling an unanchored group
(`e1bbe4b`). Actual failing and passing process regressions are preserved.

The [setup guide](claude-code-setup.md) describes configuration composition,
legacy migration, diagnostics and removal. The selected dashboard header now uses
the same managed activity quality/health as the sidebar (`2c432e0`). Native GUI
synthetic evidence and the blocked real-Claude trust boundary are recorded in
[the GUI review](../research/claude-reporting-acceptance/task7-gui/review.md).
That trust statement describes the first attempt; the authorized native follow-up
below resolved it and cleaned the fixture. This does not certify native provider
totals or final settling. Full release gates
remain open until all required provider/platform evidence is available.

Automated final checks passed586 workspace tests (11 intentional ignores),
workspace clippy and formatting. The separately executed
[50-session capacity check](../research/claude-reporting-acceptance/task7-load/scope.md)
passed its measured subset:30,000 updates/60s,3,000 stale rejections, five helper
failures, control p99=1.205ms/max=2.822ms and sampled RSS growth1.354GiB.
Internal queue counters remain unmeasured. The corrected selected-header quality
was recaptured in the native GUI with synthetic reports; actual Claude trust
approval and full provider/platform release gates remain open.


2026-09-10 authorized native follow-up: trust approval completed; two native Claude turns, observed activity, partial metrics, cost comparison, detach/reattach and exit0 verified. All recorded fixture processes, socket and disposable directory cleaned. See `research/claude-reporting-acceptance/task7-gui-native/review.md`. At that checkpoint, provider completeness and Linux gates remained open; the current Linux status is recorded below.

## Remaining-work checkpoint, 2026-09-10

The [remaining-requirement matrix](../research/claude-reporting-acceptance/remaining-matrix.md)
is the current row-level record of implementation, tests, revisions, platforms,
native evidence, and gaps. The dated [remaining-work plan](../plans/2026-09-10-claude-code-remaining-work.md)
tracks the same evidence at task level without declaring the broader milestone
complete.

Requirement reconciliation is complete. Application queue saturation, overflow,
automatic helper-loss propagation, and the macOS 50-session load passed. That work
records configured listen-backlog ceilings of 128 on macOS and 4096 on the hosted
Linux runner. It does not measure kernel socket-buffer memory or prove a full heap
maximum; those Task 2 exclusions remain open.

Private PR 55 run 34518312559 passed all three jobs at exact revision `d12abda`.
The Linux suite passed 607 tests with 11 intentional ignores across 23 binaries;
Linux clippy and formatting passed, and its doctest command executed zero tests.
The isolated Linux capacity case passed 30,000 reports, 3,000 stale rejections,
five collector failures, the original latency/RSS thresholds, application queue
bounds, and cleanup of all 50 helpers and 50 PTYs. See the
[reviewed hosted results](../research/claude-reporting-acceptance/linux/hosted-34518312559/results.md).
The earlier local Docker interruption is historical. A read-only Docker information
check timed out after five seconds on 2026-09-10, so its task-owned container and
image cleanup remains unverified. No shared-service restart was attempted.

The integrated macOS all-features suite at `d12abda` passed 633 tests with 11
intentional ignores across 24 binaries; clippy and formatting passed and doctests
executed zero tests. The contemporaneously clean
[integrated native smoke](../research/claude-reporting-acceptance/native-integrated/results.md)
passed two actual turns, full 62-column uncertainty labels, detach/reattach, exit,
and cleanup of all 22 PIDs and 16 process groups.

Native supported-route evidence is split between
[native-remaining](../research/claude-reporting-acceptance/native-remaining/results.md)
and [native-final](../research/claude-reporting-acceptance/native-final/results.md).
The latter records a real invalid-model `StopFailure` as Error/Observed and a
following valid Sonnet turn as Idle/Observed with actual output on the same
binding. It also retains the split-pane clipping failure from `ba7d560`. The
reviewed correction through `7ec5634` passed the [final single-build native smoke](../research/claude-reporting-acceptance/native-corrected/results.md): two turns, full uncertainty labels at 62 columns, detach/reattach, exit, and cleanup.

The reviewed [provider capability table](../research/claude-reporting-acceptance/provider-capabilities.md)
did not find sources that certify settled completion, complete accounting, a
final-source boundary, or the wider conversation transitions. Tasks 7 and 8 are
therefore blocked rather than implemented. Same-ID compaction has a runtime
regression, and native compaction observed explicit zero current usage. A native
null-current-context observation remains open, as do broader transition forms.
These gaps preserve Partial usage and Observed activity; no evidence here supports
Complete accounting or Confirmed settling.

PR 55 remains open, draft, and mergeable. Run 34520248256 passed all three jobs
at documentation head `c2fb7ea`; the PR has not been merged or released. Task 4
is complete at `d12abda`, and independent final contract review plus documentation
closeout are complete. Hosted queue measurements exclude kernel socket-buffer
memory, and sampled RSS does not prove a full heap maximum. A targeted
[native child run](../research/claude-reporting-acceptance/native-child/results.md)
proved that real child `PreToolUse`, `PostToolUseFailure`, and `SubagentStop` leave
root binding, activity, health, and usage unchanged. An arbitrary child API
`StopFailure`, complete child accounting, and native null-current-context timing
remain unverified.

## Part 2 explicit UUID resume, 2026-09-10

The narrow initial separate-token `--resume <canonical-lowercase-UUIDv4>` form is
implemented and independently reviewed at `9585271`. It preserves native argv,
requires matching root resume intent, and permanently closes admission on a
contradictory initial startup. Fresh positional prompt support remains intact.
The [integrated native run](../research/claude-reporting-acceptance/native-resume-integrated/results.md)
from that clean tracked revision imported one old recognized usage row and added
one new row, retained Partial / Conversation and Observed labels, exited normally,
and verified all recorded processes/groups and disposable paths removed.

Local macOS gates passed 639 tests with 11 ignored, Clippy and formatting; the
doctest command succeeded with zero doctests. Earlier probe failures and the
interrupted disk-full run remain in the [local gate record](../research/claude-reporting-acceptance/part2-final/results.md).
The full milestone remains blocked by the other provider-source and memory proof
gaps. Continue, alternate resume forms, and in-process transitions remain disabled.
The supported executable remains exactly 2.1.267; default 2.1.268 is unsupported.

## Part 3 exact 2.1.268 and short resume, 2026-09-10

The earlier Part 2 statement above records that checkpoint. The current exact
allowlist is 2.1.267 and 2.1.268. Fresh launch and separate-token
`--resume <canonical-lowercase-UUIDv4>` are supported on both versions. Exact
2.1.268 additionally supports separate-token `-r <canonical-lowercase-UUIDv4>`;
2.1.267 does not. OVRCR preserves either resume argv and requires the same matching
root `SessionStart(source=resume)` before binding.

Adjacent 2.1.266 and 2.1.269 remain unsupported. Equals syntax, missing or
noncanonical UUIDs, names/search/picker selection, continue, fork, and in-process
switching remain unavailable reporting paths. The 2.1.268 evidence does not change
Partial conversation usage, Observed activity, or the absence of a provider
final-accounting boundary.


## Codex source gate, 2026-09-11

Optional desktop alerts reuse the accepted Codex root Ready observation and its
binding, generation, turn and activity revision. They default off and belong to
the active dashboard, with no replay after attachment or reconnect, including for
terminals that are selected or shown in visible panes. An independent optional
[ready sound](dashboard.md#ready-sound) reuses that same selection; its host
playback results are in the [issue 60 evidence](../research/issue-60-ready-sound/README.md).
Explicit [unread acknowledgement](dashboard.md#unread-responses) is a separate
server-owned state for the latest managed Codex Ready observation. It survives
dashboard reconnect, Busy and reporter loss, and is cleared only by an explicit
acknowledgement naming that observation or the end of its terminal/server
lifetime. It does not change activity, quality or health. Neither feature adds
metrics or Confirmed completion. See [dashboard
controls and host requirements](dashboard.md#desktop-notifications) and
[notification-specific acceptance](../research/issue-59-desktop-alerts/README.md)
for current platform results; earlier hooks evidence below does not establish
desktop delivery.

The separately authorized [targeted invalidation follow-up](../research/codex-reporting-acceptance/history-invalidation.md) reproduced native backtracking into a new conversation with no observation hook until the next prompt. Same-ID paginated revert has a distinct shutdown/reload path. The remaining gap is continuous foreground identity, not ordinary root-token parsing; no source gate or product capability is marked complete.

Codex CLI 0.153.0 now has an implemented hooks-first adapter and print-only setup/doctor for the separately approved **last observed root turn** milestone. **Exact 0.153.0 hooks-only support passed acceptance at reviewed revision `56b84f9`.** Authenticated root prompt → Busy, matching Stop → ResponseReady/Observed, Interrupt → Idle. The last Ready observation persists across dashboard reconnect while the server remains alive, until replaced by new observed activity. If reporting becomes Unavailable, the snapshot retains its last activity with Unavailable health; this historical observation does not establish current readiness. The next prompt may rebind after the previous turn closes. This is not task success or continuous selected-history tracking. See [setup and exact launch grammar](codex-reporting-setup.md) and [native Task 2 evidence](../research/codex-response-ready-acceptance/native-task-2/README.md). Codex 0.158+ runs those hooks from a detached managed app-server daemon; OVRCR managed launches refresh that daemon and trust nested reporters in the native process tree so Ready/Input delivery is not dropped by stale `OVRCR_AGENT_SOCKET` or direct-parent attribution.

That acceptance used a disposable fixture. Operational preparation and host installation are separate: use the [installation and rollback runbook](codex-ready-installation.md) to select the reviewed binary, preserve hooks and native trust, and transition an existing server only after its live sessions have been deliberately closed. The managed `ovrcr agent run codex -- codex` launch inside an OVRCR terminal is required; a raw Codex launch remains untracked even with supported configuration.

Doctor reports exact executable compatibility and, when supplied, the presence of expected commands in one TOML file. Its accepted release status does not certify the installed binary, running server or user setup. Effective configuration, native trust and delivery remain unverified until the installed path receives native acceptance. A prepared binary or hook draft must not be recorded as installed or trusted.

The authorized [issue #58 installed-native check](../research/issue-58-ready-install/completion/README.md)
used `/Users/xlyk/.local/bin/ovrcr` from the reviewed PR #56 source, whose tree
matches merge `cadb7a3174701e1666fa0562ebb714593f9d84f3`. Exact native Codex 0.153.0
produced distinct assistant output, Ready/Observed, next-prompt Busy, interrupted
Idle, same-process dashboard reconnect and exit 0 with retained activity plus
Unavailable health. Only the five new reporters were trusted through native
Codex. Existing JSON handlers and plugin hooks retained their needs-review status;
this is preservation evidence, not a claim that those unrelated hooks dispatched.
All recorded fixture processes/groups and disposable runtime paths were removed.
The normal server transition and current delivery gates are separate in that record.

The [broader source contract](../research/codex-reporting-acceptance/source-contract.md) remains blocked: retained evidence cannot reliably invalidate continuous foreground identity when native backtracking changes lineage before its next prompt. That earlier Task 1 stop gate and prohibition on Tasks 2–3 concerned the broader metrics/continuous-identity plan; it does not block the separately approved hooks-only scope. No rollout collector or safe ongoing metrics route is claimed. Historical probes and failures remain retained.

Initial resume/picker/fork and broad transitions remain disabled for reporting. Missing ending hooks or API errors cannot imply Ready and may leave Busy; a new prompt while an old turn remains open disables reporting. Approval Input requests are available from verified root `PermissionRequest` (`approval:{turn_id}`) with close on the next attributable root activity; questions remain a named blocker (`unavailable_no_distinct_surface` — no distinct question open/close surface). Context, usage and cost remain unavailable in this milestone. Linux TUI/synthetic app-path tests are the merge bar for Ready and approval Input alerts; native Codex CUA smoke is required before merge. No Linux native GUI acceptance or concurrent provider-launch throughput is claimed. Ordinary native Codex behavior is preserved. Existing Claude acceptance does not cover Codex.

The [macOS native acceptance](../research/codex-response-ready-acceptance/native-final/README.md) verified the actual setup output, background Ready, same-process reconnect, repeated turns, root/child separation, next-prompt backtrack rebinding and retained Ready with Unavailable health after exit at 50 columns. Metrics remained null; unchanged interruption behavior retains Task 2 evidence. The [final verification record](../research/codex-response-ready-acceptance/final/README.md) records all four hosted CI jobs passing in run `34570470022`: 663 macOS and 637 Linux tests, 14 intentional ignores per platform, plus separate passing capacity and memory gates. Linux coverage is automated. Local full-suite failures and the failed native input attempt remain retained; neither is presented as a pass. The accepted native retry and complete task-owned process/socket/temporary-data cleanup are recorded separately.

## Pi 0.85.1 evidence, 2026-09-13

Pi 0.85.1 was installed locally and its extension declarations were the research anchor for
this slice: the in-process extension host, the `session_start` / `agent_start` / `agent_end`
/ `agent_settled` / `session_shutdown` events, and the session manager's session identity.
Tested versions are evidence, not an allowlist; the managed launch does not probe the
executable at all and does not pin a release, so ordinary Pi upgrades stay enabled. Doctor
still runs the bounded `--version` probe as a diagnostic and reports `probe_status` and
`version_status` from it.

Ready quality is `Confirmed` because `agent_settled` has a single emission point after
retries, automatic compaction and queued continuations have finished, so it marks the real
end of a response cycle rather than one leg of it. `agent_end` carries only the cycle
outcome discriminant and never creates Ready.

The automated proof is the Node event host (`tests/fixtures/pi/pi_host.mjs`, exercised by
`node --test tests/pi_reporting_extension.mjs`) and the managed lifecycle test
`pi_managed_extension_reports_busy_ready_error_idle_and_fences_producers`, which drives the
real extension through the real CLI, PTY, private socket and receiver. The Dashboard slice is
covered by `pi_ready_alerts_once_creates_unread_and_explicit_review_clears_only_presented`
and, for input requests, by
`pi_input_request_shows_waiting_alerts_once_and_restores_the_underlying_activity`, which
drives real `ui_prompt_start`/`ui_prompt_end` callbacks through the shipped Dashboard.
Carrying the input request on `AgentSnapshot` bumped `PROTOCOL_VERSION` to 9; turning it
into a bounded set for #95 bumped it to 10.

Session changes and recovery (#91) add no wire change. Pi replaces the extension factory on
a session replacement and on a reload, so a replacement is an observed shutdown followed by
a new producer whose `session_start` carries `previous`: the session id parsed out of the
previous session file's name, never the path and never anything inside it. Pi 0.85.1 names
that file `<timestamp>_<session id>.jsonl` (`dist/core/session-manager.js`:
`` `${fileTimestamp}_${this.sessionId}.jsonl` ``, the same shape for new and fork) while
`getSessionId()` — what the extension binds — is the bare id, so only the trailing id
travels and a name no bounded id can be read out of is no expectation at all. `session_tree` is
reported as `cycle_invalidated` with Pi's own `isIdle()`; compaction and the `session_before_*`
events are deliberately not subscribed to, so they cannot move the binding. The pause reasons
(`source_gap`, `producer_replaced`, `source_overflow`, `transition_mismatch`) and the forced
rebind that ends them are receiver-side; the extension's own recovery control is
`pi.registerCommand("ovrcr-reattach")`. Shutdown and reload share one absolute helper-drain
deadline: the queue is discarded and only the final frame is delivered. The automated proof
is `pi_replacement_binds_the_foreground_conversation_and_rejects_the_retired_producer`,
`pi_source_gap_pauses_then_recovers_at_the_next_boundary` and
`pi_reattach_command_recovers_a_paused_reporter`, driving the real extension through the real
CLI, PTY and receiver, with a dropped frame injected by `OVRCR_TEST_DROP_SEQUENCE`.

Tested versions: 0.85.1 (opt-in installed-Pi test, 2026-09-13). Doctor vocabulary is defined
in [pi-reporting-setup.md](pi-reporting-setup.md).

Native acceptance against an installed Pi, including a real extension dialog on screen
with its WaitingInput indicator and a delivered input-needed alert, is not claimed here
and is tracked by #92. The
opt-in `installed_pi_managed_launch_binds_the_real_session_and_stays_idle` test is `#[ignore]`
and requires `OVRCR_TEST_PI_EXECUTABLE` plus an isolated `PI_CODING_AGENT_DIR`; it never
touches the user's `~/.pi`.

## Oh My Pi 18.1.19 evidence, 2026-09-13

Oh My Pi 18.1.19 is a Bun-compiled binary; the bundled sources were inspected directly
because no extension type declarations survive compilation. The extension loader imports
`.mjs` with relative sibling imports, which is why the shared delivery module
(`ovrcr-reporting-transport.mjs`) is materialized beside the provider extension, and an
explicit `-e <path>` merges with discovered extensions (`--no-extensions` is never passed).
Tested versions are evidence, not an allowlist; the managed launch does not probe the
executable at all and does not pin a release. Doctor still runs the bounded `--version`
probe as a diagnostic and reports `probe_status` and `version_status` from it.

The lifecycle differs from Pi in three ways, and the extension normalizes all three onto Pi's
vocabulary rather than changing the receiver: there is no settled event, so an `agent_end`
whose `willContinue` flag is absent is followed by a synthetic `agent_settled`; a
`session_switch` (`new` / `resume` / `fork`) happens in place on the same extension instance,
so it re-announces the conversation with `session_start`; and `ctx.mode` is `"tui"` only for
the interactive terminal UI, so task and advisor children, which run in this same process
with mode `"print"`, are inert. Ready quality is `Observed`, never `Confirmed`: the
`session_stop` hook may continue after a clean end, so the end notification does not exclude
every late continuation. Cycle numbering is per extension instance rather than per
conversation, because an in-place switch would otherwise repeat an `<instance>:<run>` cycle
identity the receiver has already published.

Ordinary handlers are awaited sequentially under a 30 s budget and shutdown handlers in
parallel under a 2 s budget, both far above the transport's 900 ms per-event deadline.

### Transitions and recovery

`session_switch` is emitted from exactly three sites, and its `reason` is exactly `new`
(from `newSession`), `fork` (from `fork`) or `resume` (from `switchSession`); every one of
them carries `previousSessionFile`. There is no `tree` or `reload` reason. A same-file
reload is `switchSession` called with the file the session is already on — `reload()` reads
`this.sessionFile` and passes it straight back — so it emits `reason: "resume"` and the
session id does not change; the branch it takes internally logs a session reload. A session
file is named `<timestamp>_<sessionId>.jsonl` and Oh My Pi parses the id back out of that
name itself (taking the text after the last `_`, minus the extension), which is why OVRCR
reads `previous` out of the file name as an id and never carries the timestamp or the
directory. Every transition first emits a cancellable `session_before_switch`, and the
`session_switch` emit is downstream of the `if (handled?.cancel) return false` that gates
it; OVRCR deliberately does not subscribe to `session_before_switch`, so a cancelled
transition produces nothing at all.

The plugin-resource refresh (`reloadPlugins`) re-resolves plugin roots, rebuilds the agents
cache, refreshes skill and slash-command state and restarts MCP servers. It emits no
extension event and never touches the extension runner, which is assigned once per process;
a factory replacement in Oh My Pi therefore only happens across a process restart, which is
a new managed launch and a new receiver. `session_tree` exists and is emitted in place with
`{ newLeafId, oldLeafId, summaryEntry, fromExtension }`, gated on
`hasHandlers("session_tree")`; `session_compact` is separate, also in place, and is
deliberately not subscribed because it preserves the conversation binding. OVRCR reports
only `idle` from `session_tree` and never the leaf ids or the summary entry.

`registerCommand(name, options)` stores a command on the extension, and a slash line in the
input resolves it and runs `handler(argumentString, commandContext)`; the command context
is the ordinary extension context plus `getContextUsage`, `waitForIdle`, `newSession`,
`branch`, `navigateTree`, `switchSession`, `reload` and `compact`, and the ordinary context
already carries `ui`, `mode`, `sessionManager` and `isIdle`. So `/ovrcr-reattach` is a real
command in 18.1.19, and it calls only the transport's `reattach()` and `ctx.ui.notify`: it
never calls `reload()`, `switchSession()`, `newSession()` or `navigateTree()`, because #88
forbids a conversation reload as a reporting-reconnection substitute.

The automated proof is three managed lifecycle tests over the real CLI, PTY, private socket
and receiver: `omp_in_place_transitions_rebind_reload_and_refresh_without_a_new_producer`
(switch, compaction, A→B→A, same-file reload with the generation unchanged, a plugin refresh
whose session summary is byte-identical, tree navigation, and a switch naming an unknown
conversation that pauses and then recovers),
`omp_source_gap_pauses_then_recovers_at_the_next_boundary` (a lost frame, the hole, a pause
that applies nothing while the native session keeps answering, and one recovery at the next
`agent_start`), and `omp_reattach_recovers_a_paused_reporter_during_an_input_wait` (doctor
reporting `paused_recoverable` and naming `/ovrcr-reattach`, the command recovering while a
question is open, and an old question's close failing to clear a newer one).

Three gaps are stated rather than hidden. There is no Oh My Pi old-producer lifecycle test:
Oh My Pi never replaces its extension factory in-process, so a second instance is
unreachable at that seam, and the fence is pinned by the shared reporter-lifecycle unit
test `admit_fences_gaps_retired_producers_and_an_unobserved_replacement`. There is no Oh My Pi
`source_overflow` lifecycle test: reaching the bound needs 257 PTY round trips, so the
doctor consequence is pinned by `a_pause_is_recoverable_only_where_the_provider_has_a_way_out_of_it`
and the mechanism itself is Pi's, already covered. There is no Oh My Pi lost-bind-receipt
test: Oh My Pi and Codex take the same receipt policy, `Reporter::bind_or_disable`, which
re-reads a lost reply inside the same call because that boundary gets no second attempt;
it is covered by the Codex lifecycle test and by
`a_receiver_with_no_second_chance_re_reads_the_lost_receipt_in_the_same_call`. Native acceptance against an installed Oh My Pi stays #97.

### Approvals and questions

An earlier reading of this binary recorded approvals and questions as unobservable. That is
wrong for approvals and was corrected on 2026-09-13 by reinspecting the installed 18.1.19
binary: it emits `tool_approval_requested` with
`{ sessionId, toolName, toolCallId, reason?, approvalMode }` and `tool_approval_resolved`
with the same fields plus `approved`, both gated on
`runner.hasHandlers("tool_approval_requested") || runner.hasHandlers("tool_approval_resolved")`,
and both carrying `sessionId = n?.sessionManager?.getSessionId() ?? ""` — so the id can be
empty and must be compared against the interactive root's rather than trusted. OVRCR reports
only approvals whose session id matches the root's, which is what keeps in-process task and
advisor children from opening or closing a root request.

Questions genuinely have no dedicated export: a search of the binary finds no
`question_opened` or `questionVisibility` surface. Decision 2026-09-13 (Kyle) was to take no
upstream change and derive questions from the ask tool's own execution lifetime instead:
`tool_execution_start` (`{ toolCallId, toolName, args, intent }`) and `tool_execution_end`
(`{ toolCallId, toolName, result, isError }`) with `toolName === "ask"`. The binary itself
reads the same signal for its own question reporting
(`e.on("tool_execution_start", r => { if (r.toolName === "ask") … })`), which is the strongest
available evidence that the tool name is stable.

**The approximation and its known false positives.** A tool-lifetime request opens when the
ask tool starts executing, which is a moment before its dialog is on screen, and it stays
open for the whole execution. So it will report a wait that is not yet visible for that
moment; it will report a wait for a question queued behind another dialog, which is
arguably right but is not dialog visibility; and if an ask execution ends without ever
drawing a dialog — a same-tick abort, or no interactive UI — the request opens and closes
without anything having been on screen. Nothing here claims dialog-level visibility, doctor
says `available_tool_lifetime`, and there is no version gate, because both events exist in
every release that supports extensions and a probe would add a failure mode without adding
information.

That third case is visible to the user, not just to the snapshot. The open and the close are
two separate publications with a helper round trip between them, and the Dashboard queues a
background **OVRCR · input needed** alert when the open arrives and only cancels it when the
close does. If the alert reaches the notification host in that window it has already been
sent, and cancellation cannot recall it. So a question that was never drawn can still raise
one background alert, and a user who walks over to the terminal will find nothing waiting.
The same window exists for an approval resolved in the same tick as its request.

Request identity is namespaced so the two surfaces cannot collide: `approval:<toolCallId>`
and `question:<toolCallId>`, with Pi's `<instance>:p<n>` treated as the `prompt` namespace.
Storage is bounded: at most 32 open requests per binding, ids unique, validated in
`ProviderReport::validate`; a producer that would exceed the bound drops the new opening
rather than disabling reporting. No frame carries the approval reason, the question text,
the answer, or any tool argument or result.

**Side effect of registering the approval handlers.** The binary's speculative-read gate
refuses speculation while any tool lifecycle handler is registered — it returns
`{ allowed: false, reason: "active extension lifecycle handler" }` when
`hasHandlers("tool_call") || hasHandlers("tool_result") || hasHandlers("tool_approval_requested") || hasHandlers("tool_approval_resolved")`.
A managed Oh My Pi launch therefore trades speculative read execution for approval reporting.

Arbitrary third-party dialogs carry no guarantee: an extension that draws its own UI without
going through tool approval or the ask tool reports nothing, and collaboration guests and the
ACP route are outside the terminal UI and so silent by construction.

The automated proof is `node --test tests/omp_reporting_extension.mjs` over the shared Node
event host and the managed lifecycle test
`omp_managed_extension_reports_observed_ready_continuations_switches_and_child_inertness`,
which drives the real extension through the real CLI, PTY, private socket and receiver.
The Dashboard slice is covered by `omp_ready_alerts_once_and_creates_unread` and, for input
requests, by `omp_input_requests_wait_until_the_last_closes_and_alert_once_each`, which drives
real approval and ask-tool callbacks through the shipped Dashboard: a denied approval, an
approval opened during a question, a partial close that keeps the wait, a reused tool call id
after its close, a foreign session id that reports nothing, a request over Ready, a switch
with one open, and reporter loss with one open. The launch
shape and per-invocation cleanup are covered by
`omp_managed_launch_inserts_its_extension_and_removes_it`.
Native acceptance against an installed Oh My Pi is tracked by #97. The opt-in
`installed_omp_managed_launch_binds_the_real_session_and_stays_idle` test is `#[ignore]`
and requires `OVRCR_TEST_OMP_EXECUTABLE` plus an isolated `HOME`; it never writes to the
user's `~/.omp`.

## Native verification, 2026-09-14

Native macOS evidence for the shipped Pi and Oh My Pi reporting lives in
[research/issue-92-pi-native](../research/issue-92-pi-native/README.md) (Pi 0.85.1; first pass
at `af41fef`, second pass at `df45b59` on an idle machine) and
[research/issue-97-omp-native](../research/issue-97-omp-native/README.md) (Oh My Pi 18.1.19 at
`ee3cc05`). Each record lists revisions, commands, the per-turn ledger, PID/PGID ownership and
the cleanup check, and keeps failed attempts. Three kinds of evidence are kept distinct and
must not be read as one another:

- Native provider evidence: the installed binary launched from the Dashboard picker inside an
  isolated `HOME` that holds read-only copies of the user's credentials, driven through the real
  managed PTY, private socket, receiver and shipped Dashboard. This is what the two records
  above contain, and it exists for macOS only.
- Automated fixtures: the Node hosts and the lifecycle suites, which run the real extensions
  and the real receiver against a fake `pi`/`omp` executable. They run on macOS and on Linux in
  CI and are the only Linux coverage.
- Open gates: native Linux runs of either provider, the isolated-host fifty-session replay, and
  hosted check counts at the verified revision are recorded as open in both records; nothing
  here claims them.

What the native runs established beyond the automated suites, per record: two distinct
responses per provider with Busy, Ready (Confirmed for Pi, Observed for Oh My Pi), explicit
review, an earlier Unread surviving new work and a stale acknowledgement rejected, with the
first session launched from the Dashboard picker and later ones created with the picker's
exact argv; a real Pi extension dialog, and a real Oh My Pi approval and ask-tool question, as
Input requests; both alert kinds with identity-only bodies on both providers, where the
Response-ready notification title was captured on Pi and Oh My Pi's Ready is evidenced by its
sound; visible-pane suppression on both; reconnect baselines and Dashboard detach and reattach
on Pi; `/new`, `/resume`, `/fork` and `/tree` on both with the expected generations; Pi's
`/reload` (an extension reload: same conversation, generation unchanged) and Oh My Pi's
`/reload` (a plugin-resource refresh: summary byte-identical); a source gap — spontaneous on Pi
under load, forced on Oh My Pi — pausing as `paused_recoverable` and `/ovrcr-reattach`, typed
into the real TUI, recovering it on both, and on Oh My Pi also on a healthy reporter; Pi
cancellation and compaction; exit, signals and owned cleanup on both. Pi's built-in dialogs
(for example the branch summary prompt) publish no request, which is the documented
extension-only boundary. Still open per record: a Pi provider retry (no cheap way to induce
one) and Pi's Ctrl-D exit; Oh My Pi's registered stop hook, compaction and advisor paths, its
fallback dialog (not reachable from the TUI), an explicit Deny row, two requests open at once,
reattach during an open wait, a same-file session reload (no slash command in 18.1.19),
Dashboard detach and reattach and the reconnect baseline, the Response-ready notification
title, and sub-frame tool-lifetime ordering; real Claude and Codex turns in the mixed-provider
step (only Pi and Oh My Pi credentials were authorised); and actual display or audibility of
alerts, which the records observe but cannot prove.

Two load-dependent limits were seen while a build ran alongside the first Pi pass (load average
above 30 on 14 cores) and did not reproduce on an idle machine: the one-second `--version`
admission probe timed out, so the launch fell back to a plain native command with no reporting,
and the 900 ms helper deadline dropped frames, which paused reporting until the next boundary.
The launch-time probe was removed in this branch after that observation: it proved only that
the executable answered within a second, which a loaded machine can deny an executable that is
perfectly fine, and a non-runnable executable still fails visibly in the native launch. Doctor
keeps the probe as a diagnostic. The 900 ms helper deadline is unchanged and remains the spec's
budget, recorded here as an operating limit rather than a defect in the reviewed code.

Isolation for native runs is by `HOME` alone. Pi 0.85.1 appears to bump the modification
time of the real `~/.pi/agent` directory at startup even under a foreign `HOME`: both records
saw it, as a correlation rather than a proof; the first Pi pass also timed the real binary
against the real home once, recorded there as an operator breach. No file inside that
directory changed in either record, and every credential copy stayed inside the fixture.
`PI_CODING_AGENT_DIR` must never be exported for these runs because Oh My Pi honours the same
variable.


## Retained Claude conversations

Issue #116 adds explicit retained-session recovery through the existing Reopen
operation. `ovrcr terminal reopen ID` runs managed `claude --resume UUID` with no
new task prompt, preserving the row/title and reference before another callback.
`recovery.conversation` reports the saved UUID; `recovery.attached` is false until
the current invocation certifies the matching conversation. Process launch alone
is not attachment. Inventory restoration does not run Claude.

The current admission range is stable Claude Code >=2.1.267; native evidence remains version-specific.
Capture requires managed launch, configured synchronous native hooks and a
certified root SessionStart with its exact transcript path. Saved configuration
includes the executable, Claude configuration directory and conservative options:
model, permission mode, named agent, file-based settings, setting sources,
strict MCP, verbose and existing explicit permission flags. Inline settings,
inline agent definitions, system prompts and other unsupported configurations
are not persisted and leave resume unavailable. Task prompts are never saved.
Native trust and approvals remain in force.

Reopen checks the recorded directory, executable, history and configuration
references. A missing resource leaves an actionable failure; Retry uses the same
UUID. Unsupported clear/resume/fork observations invalidate recovery through SQLite,
retaining the prior UUID only for context. Reporting failures and missing
callbacks alone preserve a valid reference. Start new conversation creates a
separate session; other provider rows remain retained with resume unavailable.
SQLite schema 5 stores provider-tagged metadata references in one shared table; schema 3 Claude records migrate transactionally. Wire protocol is 21 (Codex references retain tag 3; session summaries include the retained working directory). Issue #117 adds display-triggered recovery through the shared launch path for eligible installed adapters; native automatic-recovery acceptance remains separate in #127. The [provider recovery contract](development/recovery.md) defines the adapter boundary for #117–#119.

Acceptance remains open. The [issue #116 verification record](../research/issue-116-claude-recovery/README.md) records the passing automated suite and controlled executable/real PTY tests, including exact arguments, repeated restart before another callback, unsupported clear before attachment, missing resources, database failures and prompt non-persistence. Native Claude continuity, Dashboard input and Linux acceptance remain unverified.

Durable acknowledgments follow successful SQLite writes. Temporary write failures retain pending work for retry and block recovery in the current owner after invalidation. If storage remains unwritable until that owner dies, the last committed reference may survive; no durability guarantee is made across an uncommitted invalidation.

## Retained Codex conversations

Issue #118 installs the Codex adapter on the shared recovery path. Exact managed
0.153.0 root startup/prompt hooks capture the UUID and matching native history
header. Reopen executes `codex resume UUID` without a prompt and preserves the
known reference before a new provider event. Initial resume reporting stays
unavailable; the Dashboard and CLI expose that limitation and never restore old
Ready/Unread. Native acceptance belongs to #128 and remains unverified here.
See [configuration, failure behavior and identity limits](codex-reporting-setup.md#retained-conversation-recovery).

## Retained Pi and Oh My Pi conversations

Issue #119 adds adapters to the same explicit `ovrcr terminal reopen ID` path.
The shared extension captures `sessionManager.getSessionId()` and
`getSessionFile()` synchronously. The shared receiver retains accepted identities
after producer fencing and binding. Pi replacement factories and OMP in-place
switches both support A-to-B-to-A changes. Retired producers cannot replace the
reference. An ephemeral session replaces the old identity but is unavailable for
resume. Reporting remains Confirmed Ready for Pi and Observed Ready for OMP.

| Provider | Source-reviewed version | Exact native reopen form |
| --- | --- | --- |
| Pi | 0.85.1 | `pi --session /absolute/native.jsonl` |
| Oh My Pi | 18.2.2 (`60c9a115`) | `omp --resume /absolute/native.jsonl` |

These are independent source/CLI checks, not native GUI acceptance. Prior OMP
reporting acceptance used 18.1.19; it does not establish recovery on 18.2.2.
The adapter checks a bounded session header for the recorded ID before launching.
It never searches by prefix, chooses the newest file, or supplies a task prompt.
Missing history, executable, original working directory or matching configuration
fails visibly. A new run retains the reference before its first callback.

History must remain unchanged by external processes between prelaunch validation
and native attachment. Concurrent deletion, emptying or replacement during that
interval is outside the recovery contract (accepted for #119 on 2026-09-17).
The native provider may otherwise start fresh, and OVRCR may accept the new
identity. No strict attachment guard is provided.

Recovery retains the absolute executable and agent configuration directory
(`PI_CODING_AGENT_DIR`, otherwise `~/.pi/agent` or `~/.omp/agent`). Supported
options are model/provider/thinking/tools/models, absolute session-dir, and
no-extensions/no-skills/no-tools. Pi also preserves approve/no-approve,
no-context-files/no-prompt-templates/no-themes. OMP also preserves approval-mode,
no-lsp/no-pty/no-rules/no-title/auto-approve. Existing trust and approval controls
are not bypassed. Initial prompts and session selectors are discarded.

Custom extensions, inline prompts/credentials, extra configuration files and
unlisted flags leave recovery unavailable rather than being silently omitted.
OMP profiles (`OMP_PROFILE` or `PI_PROFILE`), `PI_CONFIG_DIR` and XDG overrides
are not supported by this adapter. Restore the original configuration before
Retry; recovery never copies credentials or environments. Configurations changed
inside provider code are outside this conservative launch contract.

[Development handoff](../research/issue-119-recovery/handoff.md) gives automated
fixture commands and separate native acceptance instructions for #129 (Pi) and
#130 (OMP). Neither native acceptance nor Linux native continuity is signed off
by this implementation.

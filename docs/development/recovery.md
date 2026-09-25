# Provider conversation recovery

This contract enables #117, #118 and #119 to develop against the shared reopen path. Claude, Codex, Pi and Oh My Pi have installed recovery adapters. Grok retains a history reference for the title path only (see [Grok history retention](#grok-history-retention)). Other providers remain unavailable; a reference or reporting binding alone never advertises support. Codex and Pi/OMP native acceptance is still required independently in #128 and #129/#130.

## Shared boundary

- `ovrcr_protocol::ConversationReference` is a provider-tagged enum of nonsecret typed payloads. `provider()`, `identity()` and `matches_binding()` define exact ownership. Add verified provider payloads as appended variants; do not use an untyped map, newest-file lookup, resume-last or stored prompt.
- `ovrcr_runtime::recovery::{supported, unavailable, validate, resume_argv}` owns adapter selection. The adapter validates its own references and constructs managed argv without spawning. `unavailable` is the metadata capability decision; filesystem checks stay in the explicit launch validation. A supported agent without a retained reference opens its native resume picker. Invalidated or mismatched references remain unavailable. Missing prerequisites for an exact reference produce a recorded failure through the existing server path.
- `Reporter::retain_conversation(reference, deadline)` uses the current certified binding. Receivers must first pass their existing producer/sequence and native-identity checks, then bind the accepted identity and retain it. A false return is not a durable acknowledgment. An accepted replacement supersedes pending retention from the prior generation. Retry operation IDs preserve the exact binding and reference.
- `Reporter::invalidate_conversation(deadline)` permanently closes recovery for an unsupported transition in the current invocation. It cancels pending retention, retries failed persistence, and preserves the existing lost-bind receipt handling. Supported Pi/OMP changes use bind + retain, not invalidation. Their nullable native history path records an ephemeral replacement as unavailable instead of leaving the old conversation eligible.
- `RetainConversation` and `InvalidateConversation` supervisor commands use the existing lease and current session/run. Retention checks the exact provider, identity and binding generation before the shared durable write. Successful receipts are cached only after persistence. A retired lease cannot change recovery metadata.
- SQLite schema 5 owns one `agent_conversations` table. Schema 3 from archive mainline and the earlier Claude draft is distinguished by table/column markers; schema 4 from the interface amendment is also accepted. Migration preserves archive disposition, exact references and invalidation flags; offline reads remain read-only. Do not add a provider-specific store. Protocol 19 appends Codex (tag 3) after Claude (tag 0), Pi (tag 1) and OMP (tag 2); protocol 25 appends Grok (tag 4); subsequent provider registrations must append rather than reorder these tags. No database migration is needed. Protocol 21 combines display-triggered recovery from protocol 20 with retained working directories in session summaries, preserving these provider tags.
- `ServerState` remains the only process owner. Reopen creates an interactive shell in the retained working directory and writes the resume command followed by a newline. Exact references retain their managed command and reporting; missing references use `claude --resume`, `codex resume`, `pi --resume`, or `omp --resume`. The shell stays usable after the command exits. Commands longer than 512 bytes are staged in the new shell's transient `OVRCR_RESTORE_COMMAND` environment variable and submitted with a short `eval`; the command is not persisted. This avoids loss in the canonical PTY input buffer before shell startup finishes. Other launches remove inherited values of that variable. All launches use the existing reopen operation, mutation lock, capacity admission, boot acknowledgment, run fencing and production spawn. Inventory reads never launch work. Current reporting resets on reopen; `recovery.attached` requires a matching provider and identity from the new invocation.

## Display-triggered recovery (protocol 20)

`Request::RecoverSession { session, expected_run }` is the automatic counterpart
to explicit `ReopenSession`. The Dashboard emits it once per displayed eligible
run using the same nonzero pane geometry as `SetView`. Sidebar inventory and hidden
pane assignments are not displays. `SessionSummary::can_auto_recover` shares the
eligibility predicate: interrupted Agent, unarchived, supported reference, no
ownership acknowledgement required and no recorded failure.

The server rechecks eligibility under the existing mutation lock and calls the
same locked reopen implementation. It records pre-spawn failures, including full
capacity, so reconnects cannot retry them. Explicit reopen remains Retry; automatic
requests cannot acknowledge stopped processes. Natural Agent exits also retain an
explicit-action diagnostic in the existing recovery failure field; they never mark
process ownership as stopped. This prevents a later reboot from making a completed
conversation look like interrupted work. Duplicate old-run requests reuse the
successor, while archive/unarchive and stale-run fences still apply. Hierarchy events
install replacement runs; late automatic receipts cannot retarget panes or clear a
new pane's error. No persistence schema or provider registration changes are needed.

Native automatic-recovery acceptance belongs to #127, separate from development.

## Codex adapter

Compatible stable Codex CLI 0.153.x managed fresh launches retain authenticated root startup
or prompt hook identity and the exact `transcript_path`, checked against the
bounded `session_meta` header. No history discovery or latest-session fallback is
used. Resume invokes `codex resume UUID` through the existing managed launcher.
The known reference remains durably associated with the new run before callbacks.
Initial resume reporting remains unavailable; `attached` remains false. Dashboard
status and CLI `reporting_unavailable` expose this limitation. A native exit before
attachment uses the shared retained failure/Retry path.

Recovery records the last authoritative hook identity, not continuous selected
history: silent native backtracking is unobservable until another startup/prompt
hook. A replacement without a verified history file supersedes the old identity
but is unavailable for recovery. Configuration is a reference, not a snapshot;
restore the same CODEX_HOME/profile and do not change history/configuration during
reopen. See [Codex recovery support](../codex-reporting-setup.md#retained-conversation-recovery).

## Grok history retention

Grok has no resume adapter: `supported` stays false, `unavailable` keeps answering
"Native resume is not available for grok" whatever the row retains, and Reopen
refuses. What a managed launch retains (#184) is a title source for #175.

`ovrcr agent run grok -- grok` admits a fresh interactive Grok >=1.0.40 launch in
the current directory: the supervisor binds a fresh UUID, retains
`ConversationReference::Grok { conversation, history }`, and only then adds
`--session-id UUID` to the native argv. `history` is the one file Grok's documented
session store keeps for that UUID, `$GROK_HOME/sessions/<encoded cwd>/<UUID>/updates.jsonl`
(`GROK_HOME` defaults to `~/.grok`; the group is the working directory
percent-encoded, every byte outside `A-Za-z0-9-_.~`). No hook, transcript watcher,
history discovery or newest-file lookup exists; a directory whose encoded name Grok
replaces with a slug simply never matches. Resume, continue, fork, an explicit
`--session-id`, headless output, `--cwd`, worktrees and subcommands run with native
argv unchanged and retain nothing. The launch reports no activity; the row shows
`agent unknown`.

The identity check, `grok_recovery::validate_history`, is fixed before any reader
exists: the first line of `updates.jsonl` is one ACP session update whose
`params.sessionId` equals the recorded UUID. Every line of that file carries the
field, so the first line is enough and nothing after it is read for identity. A
missing file, a first line that is not JSON, or another conversation's id fails
closed. The title path (#175) must run this check before reading any text, take
`user_message_chunk` and `agent_message_chunk` text only, and never read
`chat_history.jsonl`, which holds the rendered system prompt and encrypted
reasoning. Native confirmation that Grok 1.0.40 creates the file for a supplied
`--session-id` is still open; the fixture in `tests/retained_sessions.rs` writes it.

## Parallel work ownership

| Ticket | Owns | Uses without duplicating |
| --- | --- | --- |
| #117 | Dashboard display trigger, request coalescing, race handling and automated tests (#127 owns native acceptance) | Existing recovery availability and reopen request; no provider detection in the Dashboard |
| #118 | Codex authoritative capture and resume adapter; Codex-specific tests | Reporter retention, generic reference store and shared reopen |
| #119 | Pi/OMP accepted identity changes in the shared extension receiver, separate native adapters and acceptance | Reporter producer fences, retention and shared reopen |

The common base includes archive prerequisite #115 from `c633103`. Use separate worktrees from the merged common base. Shared enum additions, adapter registration, protocol snapshots/version and capability documentation require a single integration owner: agree appended variant order and land each registration sequentially. Provider modules and their tests can proceed concurrently. Do not independently redesign the shared contract or persistence schema. #117 needs no changes to adapter registration.

Native Claude continuity and macOS GUI input from #116 are verified on `8347563`; automated macOS/Linux, capacity and memory checks passed on that revision. See the [acceptance record](../../research/issue-116-claude-recovery/native-continuity-20260917/README.md) for the two-restart proof and platform boundaries. Native Linux GUI was not exercised. Downstream tickets require their own provider acceptance; this evidence does not authorize additional paid runs or a merge. The existing limitation for persistent storage failure followed by owner death before invalidation commits remains unchanged.

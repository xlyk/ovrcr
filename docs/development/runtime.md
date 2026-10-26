# Runtime ownership and lifecycle

Notification navigation lives in `server::navigation` and the existing
`ActiveDashboard` slot. Startup caches a bounded SHA256 identity of the actual
CLI executable and creates a fresh Server-lifetime token. A connect-only callback
must match that context and an unarchived retained run. One pending offer is
stored per current owner with a 1.2-second budget. Lifecycle admission uses
`try_lock`; a busy or stopping owner yields a no-op instead of deferred work.
Confirmation is queued on the sole Dashboard writer while the mutation lock
still excludes lifecycle changes. The Dashboard compares its current hierarchy
before applying; the Server revalidates owner, deadline, callback connection and
run again on the applied receipt. Only that receipt may expose current socket
peer PID/birth/terminal context for native parent activation. This path never
uses terminal attach/select or launches a managed process.

- Give each piece of state one authority. Avoid parallel stores for selection, geometry, readiness, request completion, or process ownership. Compatibility helpers must delegate to the same implementation.
- Respect the existing lock order and spawn/registration boundary. Keep owner verification and state publication atomic. `Session::spawn_registered` is the only production spawn; it publishes the session before the PTY reader and child waiter start. The two `cfg(test)` hook constructors publish at that same point with a no-op register, so no spawn skips publication. Delayed cleanup from an old connection must leave a replacement owner's state intact.
- Preserve the synchronous dispatcher and sole socket writer. Socket writes must not block the parser dispatcher. Bound queues and retained state; preserve control responses and lifecycle events or disconnect explicitly when they cannot be delivered. `reporting_channel` is the only queue constructor; its monitor argument turns the accounting on for the `acceptance-diagnostics` build and for tests that assert on it, and nothing else varies. The `after_send` / `before_wait` ordering hooks belong to that accounting path and fire only on a monitored channel; an unmonitored one is a plain `mpsc::sync_channel`.
- Apply final PTY output before exit notification. Detach, hide, and pane close change subscriptions; they must not terminate sessions.
- Signal only process groups whose ownership the current runtime or fixture established. A discovered or remembered PID is not ownership proof. Never use broad process-name cleanup.
- Live PTYs and screens belong to the running server. Detach/reattach preserves the current run. Reopen starts a fresh process in the same durable session row; never restore a PTY, output, or activity from metadata.
- Preserve atomic persistence and failure boundaries. A failed write must not be reported as committed, and a partial external side effect must not be described as rolled back.

The active dashboard (identity, queue, view, geometry) lives in `server/dashboard.rs`. Prove ownership with `ActiveDashboard::owns`; never compare identities inline.

`SessionStore` is the durable authority for session identity, run intent and
recovery metadata. Persist run intent before spawning; hold `mutation_lock`
through live-slot admission and publication. Only Running and Paused runs count
toward fifty slots. Reject stale run-tagged events, input and completion paths.
A known pre-process launch failure is retryable; a failure after process creation
must not claim the process or its descendants are gone.

Persist the native OS boot identity with every run. Only validated, different
boot identities establish prior-process exit without explicit acknowledgement.
Missing, malformed or same-boot evidence fails closed. Never signal remembered
PIDs or infer descendant exit from the leader or server disappearing.

Natural exit retains ownership uncertainty: process-group or TTY disappearance
is not proof about background jobs. Keep `stopped` as the durable authority;
do not add process-table polling or a parallel cached proof. An already-exited
termination result is not a controlled stop. Only a verified stop of the current
owned run may certify that control outcome; failed discovery, races and partial
termination retain uncertainty. Explicit acknowledgement is fenced to its
captured session and run.

The Server records its own decisions as Events (ADR 0009). Each event is a
component (`titles`, `settings`, or `quota`), a time, the session or provider
concerned, and one line that follows the Quota reason rule: no prompt, transcript,
credential, account identifier, or native body. The in-memory ring holds 2000
events and drops the oldest. Every new event is also appended to `events.jsonl`
in the instance directory, mode 0600, one JSON object per line. At 5 MB the
file rotates once, to `events.jsonl.1`; the rotation renames the whole file, so
a line is never split, and older lines are not rewritten. stderr is unchanged.
`ovrcr events` reads the ring through a Server request. The Dashboard popup that
shows the same ring is a separate change and is not implemented here.

Dashboard create/remove/close (CreateWorkspace, CreateWorkspaceWithLaunch,
CreateSession, CloseTerminal) are accepted immediately on the Dashboard
connection and run as one Server Lifecycle job on a dedicated worker under
`mutation_lock` (ADR 0010). Input and SetView keep flowing on the reader while
the job runs. Assigned runs receive an immutable input-ownership marker before
Session publication; generic input checks that marker without taking the
lifecycle mutation lock. Explicit assigned-human input has a separate gate:
retained authority, child state, writer, active Dashboard slot, acknowledged view.
It validates the captured child epoch/run/view and current Human owner, then
persists a content-free human-input revision before bytes. It never takes the
lifecycle mutation lock. Takeover/return use the same durable per-child control
record; neither changes immutable assigned membership. Scoped delegation input
checks its captured control epoch and current parent/child bindings under their
Session state locks through the PTY write. Parent effects additionally hold the
retained workflow owner through the final publication/spawn/write fence, validate
the captured controller epoch, and authenticate the current in-memory source key.
Source registration and Reporter disconnect retirement follow retained → Session
lock order. Explicit parent transfers use the same retained row and never reset
child control or immutable assignment identity. A second Lifecycle job is refused. Completion is a typed
`LifecycleCompleted` event correlated by the accepting request id, plus
`HierarchyChanged` as today. CLI-only RemoveSession and DeleteArchived stay
synchronous on the caller.

Workflow cancellation lives in that same durable WorkflowRecord. Commit the
bounded complete capture, fixed member identities/gaps and child interrupt
intents atomically before effects. A tombstone gates new and resumed Accepted
dispatch after discovery and survives parent transfer. Native interrupts use
the exact current child writer with an Unknown receipt and advanced child
barrier already durable. Confirm only that barrier with fresh same-turn Pi
agent_settled evidence under current parent/child locks; preserve historical
delivery uncertainty and unrelated input or attribution fences. Wait aborts
terminate only their observer helper. No operation archives or removes resources.


Delegated archive commits an exact-run Archive barrier and unknown stop receipt
before capability revocation, then releases mutation/retained/Session state for
the existing owned termination chain. Its termination mutex is acquired with
try-lock before ordered Session state and released before completion relocks.
Stop proof plus pending receipt commit together; archive/run advancement plus
final receipt and exact barrier retirement commit together. Retained caches
publish only after those commits. Final filing pins the current parent source
through commit, including continuations. An explicit continuation consumes its
protected slot/bytes only on successful filing and never performs another stop.
Same-resource lifecycle changes respect the barrier; unrelated input/view remain
available during the stop. Archive preserves private evidence and workspaces.

Coding delegation uses the same already-locked combined workspace/managed-launch
body. Record proposed repository/path/pinned base before Git; save created Git
generation before registration and registered resource before native launch.
Revalidate the registered repository root's existing Git generation at coding
intent, effect and current-capture gates, with Git outside retained/Session guards.
A NoProcessStarted proof is independent of worktree existence. Final rejected
launch receipts stay immutable; only one explicit linked successor may reuse a
verified no-process/no-input workspace. Unknown ownership never permits adoption.
Candidate capture runs without retained/Session locks. Its in-memory bytes gain
a discoverable filename only after the existing child-authenticated publication
intent binds the exact enriched result digest. Recovery uses original stored
bytes and selected version/contract; it never captures a newer checkout.


Dependency edges stay in the existing WorkflowRecord. The parent declares the
bounded graph; ordinary Prepare/Launch/SendInitial retain scheduling and effects.
Prepare authenticates selected original candidate/acceptance/reviewer files and
pins a private complete input baseline. Lifecycle workspace registration precedes
compatible snapshot materialization, and the existing creation gate rechecks
parent/source authority before spawn. Initial input rechecks actual baseline
content and ancestor candidate/review versions. Followup and acceptance reject
stale dependency witnesses; collection preserves the original evidence and
reports the gap. Graph revision changes do not interrupt existing children.

Cursor cloud previews require a positive managed reservation and invocation
channel. A descendant admission probe is authenticated against that same
supervised native tree; direct adapter invocation fails closed. The shared
Reporter owns lease teardown. Each Session owns a unique private cloud scratch
root, removed by its waiter after PTY drain and process-group exit, so killing
the adapter cannot leave its bearer file behind or erase a replacement run's
root. The SDK Bridge receives an environment allowlist without OVRCR credentials.
Git, Keychain, archive extraction, discovery and catalog I/O run in the adapter
under a thirty-second preflight deadline, outside Server input/lifecycle locks.

Preflight Git, Keychain and extraction helpers inherit the adapter's supervised
process group. A helper timeout reaps the direct child and fails preflight,
exiting the adapter; the surviving invocation supervisor then kills the whole
group, including helper descendants. Helpers must not create separate groups
whose only cleanup owner would disappear with the adapter.

Cursor cloud creation and Send share one Server/SessionStore operation record. Authenticated Reporter
commands require the current CursorCloud reservation; intent/private publication,
dispatch claim and exact handle receipt are separate durable boundaries. Claim
and local closure serialize under the retained mutex; the adapter releases those
locks before bridge I/O. Local Discard is a closure, never a remote-cancellation
receipt. Archive preserves unresolved bytes; local row deletion requires explicit
Discard, including for acknowledged handles. The original Session owns the operation across restart; no handle
restoration, automatic creation or uncertain Retry is permitted. Send requires the
original durable creation receipt, immutable body/options and a distinct key/claim;
exact run retention precedes payload retirement. Busy input is rejected for manual
resubmission. Exact run observations use the existing Reporter/revisions; stream
loss remains unknown and no newest-run lookup supplies identity. The
accepted pending-submission exception permits immutable request bytes only in
confined owner-only plaintext files, readable by same-user programs and backups;
credentials stay outside them. Creation payload retirement follows durable exact
identity/receipt; Send payload retirement requires its exact durable run ID.

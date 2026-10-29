# Cursor cloud prompts, cancellation and results

On macOS, ordinary **Agent** creation lists `cursor-cloud:ACCOUNT` for each valid
named account in `dashboard.toml`. Local `cursor-agent` remains separate.
Linux explicitly reports this provider unavailable.

`just install-release` installs the native adapter as part of `ovrcr` and the
pinned official archive on macOS arm64 and x86_64. `just run` refreshes both too.
To install only the dependency, run `just install-cursor-bridge`; it downloads
only official release v1.0.40 and verifies the host architecture's SHA256.
For an offline install use `just install-cursor-bridge /absolute/official/archive.tar.gz`.
Release/development installs also accept `OVRCR_CURSOR_BRIDGE_ARCHIVE` for that
explicit archive. A mismatched/missing supplied archive refuses installation,
preserving the previous dependency. No downloaded code executes during install.

The archive lives at `~/.local/lib/ovrcr-cursor-sdk/1.0.40/bridge.tar.gz`, beside
`artifact.json` with SDK/bridge/protocol/hash metadata. Notification Bridge
publication owns a different tree, `~/.local/lib/ovrcr`, and cannot replace this
dependency. Copy the installer's printed **absolute** path into each profile;
there is no PATH lookup, ambient endpoint, automatic settings edit or `~`
expansion in `bridge_archive`. Existing explicit official archive paths remain valid.

Configure a nonsecret profile in the existing settings document:

```toml
[[cursor_cloud_accounts]]
name = "work"
keychain_service = "ovrcr.cursor-cloud"
keychain_account = "work"
model = "composer-2.5"
repositories = ["https://github.com/acme/app"]
owners = []
bridge_archive = "/Users/YOU/.local/lib/ovrcr-cursor-sdk/1.0.40/bridge.tar.gz"
```

Store the selected Cursor API key as a generic password under that exact macOS
Keychain service and account. No key belongs in this file, argv, logs or
screenshots. A profile selects a credential and policy; it does not attest which
remote billing identity owns the key. Missing/wrong keys, unavailable models and
policy failures refuse validation. There is no ambient-login or model fallback.
Repository policy accepts exact canonical GitHub URLs, or explicit GitHub owners.

`ovrcr agent setup cursor-cloud --print` and
`ovrcr agent doctor cursor-cloud --json` report configured profiles plus passive SHA256 checks of the installed archive
and every configured archive, without reading credentials, extracting/executing
code or making cloud calls. `bridge_dependency` reports the pin, absolute installed
path and `verified`/`invalid` results. Missing or altered archives remain invalid;
repair with the installer and recheck the configured absolute path. Diagnostic
success means a report was produced; inspect each status. `runtime_checked` and
`release_ready` remain false: a checksum establishes artifact identity only. `--settings PATH`
selects an existing settings document for those diagnostics only. The ordinary
managed launch grammar is `ovrcr agent run cursor-cloud -- ACCOUNT`; running it
outside a managed OVRCR Agent session refuses admission. To create that managed
session from the CLI, use the same wrapper inside `terminal create` (or `new`):

```sh
ovrcr terminal create --project PROJECT --workspace BRANCH -- \
  /absolute/path/to/ovrcr agent run cursor-cloud -- work
```

The retained session keeps the selected `cursor-cloud:work` identity, as ordinary
Agent creation does. Account configuration and launch validation still apply.

The preview validates the workspace origin and published HEAD, then the selected
key's repository/model access through one owned official SDK Bridge. It shows
account, repository, full commit SHA and explicit model. Local edits are visibly
excluded. Nothing is implicitly uploaded, committed or pushed. Enter closes the
preview and its owned bridge. Type `prepare` and Enter to allocate a local cloud
handle through the pinned SDK. The acknowledgement is a local handle, not proof that a backend agent/run exists.
After preparation, enter a prompt to Send on that retained conversation. Its first
Send creates backend work. The terminal names its exact accepted run, streams
recognized assistant content, and reports Ready only after source-tool proof,
matching terminal result/done/end receipts and an independent exact GetRun.
GetAgent must confirm the same conversation, cloud runtime and repository.
A later idle prompt uses the same account/conversation with a new Send key and
run. A failed run reports Error; a disconnected or unrecognized stream reports
observation unknown, since remote work and charges may continue. After a transport
interruption with an exact durable run receipt, the original owned bridge may make
at most two ObserveRun attempts. The first starts at the beginning and suppresses
already-applied Send events; the second resumes after durably acknowledged exclusive
ObserveRun progress. Send and ObserveRun offsets are stored separately under the
original account/conversation/run and stream origin in shared SQLite. Repeated
offset identities suppress only matching events; equal text without a Send offset
remains a distinct occurrence. Beginning ObserveRun replay consumes the count of
already-applied occurrences, preserving repeated equal-content output.
Replayed completion cannot create another Ready/Unread. Invalid identity, malformed evidence,
missing/changed offsets or failed progress persistence stop observation. No Send is
replayed, and no fresh bridge is started for this bounded recovery. Prose never
creates a cloud terminal request. Correlated terminal questions/approvals, uncertain
CreateAgent/Send Retry, and fresh-process active restoration are unavailable and
not planned under the current scope. Exact known-run recovery is described below.

Send and ObserveRun offsets are opaque exclusive strings scoped to their stream
origin and retained separately. Preserve each token exactly; do not parse or order
it as a number. A valid empty frame with no event or offset is a keepalive: it
does not create a run receipt, advance either cursor, or produce Ready/Unread.
Informational `task`/`usage` events and future informational SDK events do not
establish agent or run identity. Before a receipt, they cannot mint a receipt or
cursor or trigger GetAgent/ObserveRun/Ready/Unread; after a run is known, any
agent/run identifiers carried by metadata are checked against that run before the
metadata advances its own stream cursor.
Foreign metadata identity fails closed. Unknown future outer frames are skipped
without progress; malformed or unrecognized evidence that could affect source,
run or result identity still fails closed.

Busy prompts are rejected visibly and the latest rejected prompt stays in local
adapter memory for manual `/resubmit` when idle. It is never a future-prompt queue
and is lost when that adapter exits. An empty idle line or `/detach` closes local
observation; an active synchronous stream processes `/detach` after it returns.
An uncertain Send refuses further prompts and offers `/detach` and the existing
local Discard control. There is no automatic retry, new conversation or newest-run
lookup. Changed profile defaults cannot rewrite retained requests.

The official standalone archive is pinned to SDK **1.0.40**, bridge **1.0.0**,
protocol **sdk.v1**; installation does not download or select a newer release.
The Darwin ARM64 archive SHA256 is
`56f7f68a43ce73daa8d93c60b35b5301feb6a351e1dabfefe9873d80d56931cb`;
Darwin x64 is
`cc1e1779e9768fc0db09a93d98dce51498e507ee26a24ae04c0dd21b21700ea6`.
The archive manifest records source/bridge commit
`863444c1b502763bc7e22777084d20d352d5cdf4`; the separately reviewed public
[docs/schema source](https://github.com/cursor/sdk-bridge/tree/1ba4fe546ea597c2f14b571b0ebe59ab1d20b70e)
is a different revision. Matching unmodified definitions are vendored under
[`vendor/cursor-sdk-bridge/1.0.40/proto`](../vendor/cursor-sdk-bridge/1.0.40/proto).
Rust uses explicit JSON translation against those definitions; there is no
protobuf generation step or generated client to claim verified. Managed launch
rechecks the actual archive before private extraction and requires runtime bridge
1.0.0/protocol sdk.v1, independently of passive doctor results. It preserves
strict discovery identity and requires advertised `agent.create`, `agent.send`,
`run.cancel`, `run.observe`, `agent.management` and `cursor.catalog` before account/catalog
requests or agent operations. Missing/malformed capability lists refuse launch;
unknown additional strings are forward compatible. Advertisements are protocol
negotiation, not behavioral proof of terminal responses, retry or active recovery.
The client uses authenticated Connect unary requests over loopback HTTP/1.1,
with redirects/proxy discovery disabled and bounded responses. Only GetVersion,
Me, ListModels and ListRepositories are used for validation; explicit `prepare`
also uses CreateAgent. Send uses the pinned Connect streaming envelope and exact
GetAgent/GetRun reads, without ResumeAgent, ListRuns or automatic PR creation.
The bridge receives its selected
key in a separate environment without OVRCR reporting or delegation credentials.

## Retention and local controls

The existing Server/SessionStore owns creation, using the same SQLite connection
as retained Sessions. Schema 24 retains the original Session/run, invocation,
Server lifetime, selected account/profile, credential generation, canonical source,
model/options, immutable CreateAgent key, dispatch claim and exact receipt. A
confined `cloud-creations/` directory in the selected instance stores the original
credential-free semantic request in owner-only private files. No API key, bridge
bearer or OVRCR credential belongs in these files or in a conversation reference.
Request files are plaintext: same-user programs, backups and filesystem snapshots
can read them. Permissions do not provide application encryption.

Preparation commits the immutable intent before private publication. The claim
verifies those exact bytes and commits before CreateAgent; failed publication or
persistence permits no creation. Closure and the claim serialize under the same
retained owner. Closure that wins blocks dispatch; a claim that wins stays
possibly escaped even if local Discard follows. Bridge I/O holds no shared
lifecycle or retained lock. Receipt retention validates the exact returned agent
ID and explicit model, then commits before retiring creation bytes. Send uses a distinct immutable body/key and claim under this same serialized
owner. Its credential generation, explicit model and acknowledged agent must
match the original creation receipt. Full prompt/options bytes live only in the
same private directory; SQLite keeps their digest and non-payload operation
receipts. A successful durable Send claim retires the prior Ready/Unread immediately;
activity stays unknown until its exact run is observed. A failed claim commit
preserves prior response authority. The exact run ID must commit before payload retirement. A crash after
backend acceptance but before that receipt remains visibly uncertain. A late
exact receipt can enrich a closed row, but cannot reopen delivery or reporting.
Each conversation is bounded to 128 retained Sends per local handle; reaching the
limit refuses another Send. Restart never silently replaces a handle.

An unresolved creation stays on its original row through exit and restart without
inventing a conversation reference, replacing its key, using a resume picker or
automatically creating another session. Archive preserves it. Explicit
`ovrcr terminal cloud-discard ID` closes local delivery and retires payloads;
it does not claim remote cancellation or resolve an escaped effect. Local row
deletion requires that Discard, including after an exact handle acknowledgement. An authenticated
late exact receipt may enrich the existing closed operation, without reopening
it. Missing rows and stale owners cannot be recreated by a receipt. Once the
owned bridge exits, restoring the same prepared pre-Send handle is unavailable on
this pin, so that unresolved creation fails closed. Fresh-process active restoration
is also unavailable and not planned. CreateAgent/Send Retry is unavailable and not
planned; exact completed-run GetRun reconciliation remains supported.

## Cancel and Discard

Type `/cancel` in the live terminal, including while a Send is busy. The Server
commits cancellation intent and closes further delivery before acknowledging it.
An unclaimed request cannot dispatch; a claimed request without its original run
ID remains possibly sent and remote cancellation unconfirmed. No CreateAgent,
Send, ListRuns or newest-run selection is used to discover a cancellation target.
Cancel alone keeps unresolved private request content.

For the exact known run, a separate durable claim admits at most one CancelRun
on that original live bridge. If the original in-flight Send supplies the missing
run ID later, that identity commits before the live attachment completes the
already requested cancellation. An RPC acknowledgement means **requested**.
Only an independent credentialed exact GetRun confirms Cancelled, Finished or
Error; the bridge's cached cancelled status is not remote termination proof.
A completion that wins the race is shown accurately. Delivery remains closed.
A failed intent/claim write permits no cancellation RPC and reports no durable
success for the failed boundary. There is no automatic cancellation retry.

`ovrcr terminal cloud-cancel ID` persists the same intent/closure for prepared,
uncertain or detached rows. Its success acknowledges local durable intent only;
the original live adapter checks this typed intent at most once per second and
services its exact retained target, including after stream loss. Partial terminal
input stays buffered; no terminal commands are injected. A failed admitted claim
is not automatically retried. A detached bridge sends no CancelRun. Saved intent survives
restart. Display/recovery never sends CancelRun automatically; explicit Retry
Cancel requires the separate supported recovery path. An explicit recovered exact-run route is described below; display never sends cancellation.

`ovrcr terminal cloud-discard ID` durably closes delivery and marks private content
for retirement before deleting canonical files and owned temporary aliases. A
cleanup failure reports an error; startup retries retirement from the durable
record. Discard never calls CancelRun and cannot undo an already claimed remote
attempt. Late exact receipts can enrich the original closed record without
restoring payloads, dispatch permission or a deleted row. Archive preserves
unresolved content; deleting the local row requires Discard first. Closing a pane
is a display action. Closing the session ends local control and archives its row;
remote work and charges can continue. Local file deletion does not erase backups
or snapshots.

## Isolated acceptance

Default builds contain no fixture override. A build with `acceptance-diagnostics`
can use `OVRCR_CURSOR_CLOUD_FIXTURE=/absolute/private-root/cloud-fixture.json`.
Add `"credential_missing": true` to simulate a missing selected Keychain key.
The directory must be owner-only mode 0700; the regular, singly linked JSON file
must be owner-only mode 0600. Its schema is:

```json
{"schema":"ovrcr-cursor-cloud-fixture-v1","bridge":"/absolute/private-root/bridge.py","published_sha":"FULL_FIXTURE_HEAD_SHA"}
```

Copy [`tests/fixtures/cursor-cloud/bridge.py`](../tests/fixtures/cursor-cloud/bridge.py)
to that path with mode 0700. An adjacent `bridge.json` contains an absolute `log`
path, and optionally a model/repository or `reject_credential` flag. The adapter
uses only a fixed fake credential and recorded publication proof in this mode.
The fixture accepts exact creation and Connect Send shapes, including delayed
run_terminal_cmd arguments and result.success. It records
method names, process IDs and resource paths, allowing exact permitted creation
counts, zero-Send assertions and owned cleanup checks.
A diagnostic `preflight_helper` executable and bounded `preflight_timeout_ms`
(up to 30000) can stall the actual bounded-helper path for forced-death/timeout
cleanup tests. Helpers inherit the supervised adapter process group.
This proves native wiring and lifecycle with a local service, not real Keychain,
Cursor access, or official-bridge runtime behavior. Production pins remain strict.

Creation diagnostics additionally accept `creation_crash_at` (`before_dispatch`,
`after_reply`, `after_ack`) or `creation_pause_at` (`before_dispatch`, `after_claim`,
`after_reply`, `after_ack`) with an absolute `creation_checkpoint` marker path.
A pause ends only when its sibling `.release` file exists, with a 30-second bound.
These controls exist only for diagnostic fixtures and never enable real keys.
The fake bridge's adjacent configuration supports `drop_creation_reply` to model
acceptance whose reply is lost; this says nothing about upstream retry guarantees.

## Result links and limits

Only explicit result branches/PR URLs are candidates. A published branch is
labelled as a branch, with no PR inferred or created. The verifier reads GitHub
through `gh api`, binds the exact repository/branch commit, and requires its merge
base with the selected source to equal that source. An existing PR must additionally
match the exact repository, head branch and commit. Unavailable credentials,
mismatched identities and ambiguous output remain visibly unverified. There is
no missing-branch fallback and no automatic PR creation. Arbitrary transcript URLs
and artifact download paths are not promoted to verified links. Result/source
qualification does not independently attest the agent's claimed tests.

The first Send asks for a correlated `git rev-parse HEAD` tool before other tools.
Only recorded pinned shell/run_terminal_cmd success shapes can confirm the selected
published SHA; assistant claims alone cannot. Later follow-ups retain that source
provenance while allowing the same conversation's work to advance. Unknown shapes
that could affect source/run/result identity fail closed; informational events are
handled as described above. Live/paid Cursor behavior, native Keychain and packaged
runtime acceptance remain separate release gates; local fixture success proves
wiring only.

Send diagnostic controls mirror creation: cloud-fixture.json `send_crash_at` or
`send_pause_at` accepts before_dispatch, after_claim, after_reply, after_ack;
pauses require an absolute `send_checkpoint` and end at its `.release` sibling
within 30 seconds. bridge.json `drop_send_reply` accepts then loses the reply;
`send_pause_file` stalls the stream until its `.release` sibling. Corruption
`send_scenario` values include cross_run, unknown_shape, bad_source, bad_call,
bad_success, missing_done, wrong_repository and wrong_snapshot. Tests use a
private Git-backed gh executable to exercise the same production link verifier
without network or real credentials; fixture evidence does not prove live GitHub.

Cancellation fixtures additionally support bridge.json `pause_after_identity` with
an absolute marker path and its `.release` sibling, `cancel_outcome` for the cached
stream status, and `cancel_snapshot_outcome` for independent exact GetRun status.
These simulate races only and do not attest real upstream behavior.

Fixture controls `drop_send_after` and `drop_observe_after` interrupt after a bounded
number of envelopes; `observe_pause_file` waits for its `.release` sibling.
`observe_scenario` exercises wrong-run, changed/missing offsets, repeated transport
loss and replayed completion. These are synthetic fault controls, not cloud proof.

## Exact-run recovery

`ovrcr terminal reopen ID` reuses the retained Session and working directory.
Use `--ack-stopped` only after confirming the previous local process tree stopped.
A cloud row with an exact acknowledged run starts a managed fresh bridge, validates
its original credential generation and current account/source policy, then reads
that exact conversation and run. Changed local model defaults do not replace the
retained options. A changed credential or Keychain reference refuses recovery:
repository access and matching profile names cannot revalidate account continuity.
A missing working directory, changed source, wrong IDs, unknown lifecycle or failed
lookup remains uncertain. Pending creation/Send without an exact run keeps its
existing operation controls; no resume picker or newest-run discovery is used.

A terminal snapshot is retained as historical reconciliation, without a new Ready,
Unread or input alert. Active runs visibly say **Active reattachment unavailable**.
Fresh-process active restoration is unavailable and not planned under the current
scope. Recovery sends no ObserveRun, ResumeAgent, CreateAgent, Send or option
mutation. Fresh-handle reuse of prior Send/ObserveRun offsets is unavailable and not
planned; no fresh observation stream is opened.

For an active exact run, type `/retry-cancel` explicitly. A fresh credentialed exact
GetRun registers the selected-key handle before the same SQLite owner commits
cancellation intent, delivery closure and a recovery cancellation claim. CancelRun
then targets that handle; a separate fresh GetRun supplies the outcome. Completion
may win the race and remains Finished. An acknowledgement alone never means stopped.
Each recovered attachment permits one admitted cancellation attempt. A failed or
still-active outcome keeps the intent; detach and explicitly reopen before another
Retry Cancel. Saved intent is never automatically dispatched. `/detach` closes local
observation. Closing a pane remains display-only; closing the Session archives its
row and warns that remote work and charges may continue. The Dashboard shows its
existing close confirmation for cloud rows even after local exit; the explicit
`terminal close ID` command prints the same warning on stderr, including in JSON
mode, before issuing close. Closing does not send CancelRun.

The original creation/Send operation identity and immutable private payloads survive
attachment and archive transitions. Recovery authority is subordinate metadata in
the same SessionStore row; only its current managed invocation can reconcile or
claim cancellation. An exact conversation already owned by another retained Session
is refused with that Session's ID. Discard and stale-owner fences remain authoritative;
recovery never grants CreateAgent/Send dispatch. CreateAgent/Send Retry is
unavailable and not planned under the current scope. Completed lookup supports
historical reconciliation only; fixture success does not establish live behavior
or replace the remaining native, live, packaging and CI acceptance.

Recovery diagnostics support `credential_changed` in cloud-fixture.json (a second
fixed fake key, never real credentials), and `recovery_runs` in bridge.json, mapping
exact fixture run IDs to lifecycle strings for a fresh bridge. `recovery_missing`
returns a lookup failure. These simulate backend state across bridge lifetimes and
prove local wiring only.

## Packaging acceptance remains open

The installed dependency and passive checks do not establish native release
readiness. [#378](https://github.com/xlyk/ovrcr/issues/378) still requires
current-head packaged Dashboard/provider acceptance, separately approved
concurrent two-account cloud proofs, required macOS/Linux CI and process cleanup
receipts. The unsupported #374/#376/#377 remainders are unavailable and not
planned, not release gates or feature passes. Retain the implemented same-owned-
bridge ObserveRun path and completed exact-run GetRun reconciliation; fresh-process
active restoration, fresh-handle old-offset reuse and CreateAgent/Send Retry
remain excluded.
The current upstream [v1.2.0 local-agent smoke procedure](https://github.com/cursor/sdk-bridge/blob/v1.2.0/docs/smoke-test.md)
uses a Cursor API key for account/catalog access and sends a model prompt through
a local agent; it is not credential-free or offline and may incur charges. OVRCR
remains pinned to SDK 1.0.40, so this upstream documentation is a separate smoke
reference, not runtime evidence for the packaged OVRCR provider. It needs separate
concrete approval. Passive checks or the zero-model local control subset cannot
count as that full smoke. Its `composer-2` sample is stale/redirected and is not
evidence that the model is currently available. Existing fixture results and the
standalone cloud POC retain their original scope.

## Stream conformance evidence

Focused managed synthetic regressions now cover opaque nonconsecutive Send and
ObserveRun tokens, empty keepalives on both paths, `task`/`usage` and future
informational events, future outer frames, exact metadata identity checks, and
receipt/cursor/Ready/Unread ordering. Capability-list forward compatibility does
not by itself establish event-envelope behavior. These fixtures do not prove real
Bridge/provider behavior or packaged native Dashboard acceptance.

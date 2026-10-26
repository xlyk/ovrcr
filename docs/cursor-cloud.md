# Cursor cloud prompts and verified results

On macOS, ordinary **Agent** creation lists `cursor-cloud:ACCOUNT` for each valid
named account in `dashboard.toml`. Local `cursor-agent` remains separate.
Linux explicitly reports this provider unavailable.

Configure a nonsecret profile in the existing settings document:

```toml
[[cursor_cloud_accounts]]
name = "work"
keychain_service = "ovrcr.cursor-cloud"
keychain_account = "work"
model = "composer-2.5"
repositories = ["https://github.com/acme/app"]
owners = []
bridge_archive = "/absolute/path/cursor-sdk-bridge-standalone-darwin-arm64.tar.gz"
```

Store the selected Cursor API key as a generic password under that exact macOS
Keychain service and account. No key belongs in this file, argv, logs or
screenshots. A profile selects a credential and policy; it does not attest which
remote billing identity owns the key. Missing/wrong keys, unavailable models and
policy failures refuse validation. There is no ambient-login or model fallback.
Repository policy accepts exact canonical GitHub URLs, or explicit GitHub owners.

`ovrcr agent setup cursor-cloud --print` and
`ovrcr agent doctor cursor-cloud --json` describe the configured profiles without
reading credentials, starting a bridge or making cloud calls. `--settings PATH`
selects an existing settings document for those diagnostics only. The ordinary
managed launch grammar is `ovrcr agent run cursor-cloud -- ACCOUNT`; running it
outside a managed OVRCR Agent session refuses admission.

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
observation unknown, since remote work and charges may continue. Prose never
creates an input/approval request. Terminal responses, Retry and active
reattachment remain blocked.

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
Matching definitions are vendored under
[`vendor/cursor-sdk-bridge/1.0.40/proto`](../vendor/cursor-sdk-bridge/1.0.40/proto).
The client uses authenticated Connect unary requests over loopback HTTP/1.1,
with redirects/proxy discovery disabled and bounded responses. Only GetVersion,
Me, ListModels and ListRepositories are used for validation; explicit `prepare`
also uses CreateAgent. Send uses the pinned Connect streaming envelope and exact
GetAgent/GetRun reads, without ResumeAgent, ListRuns or automatic PR creation.
The bridge receives its selected
key in a separate environment without OVRCR reporting or delegation credentials.

## Retention and local controls

The existing Server/SessionStore owns creation, using the same SQLite connection
as retained Sessions. Schema 22 retains the original Session/run, invocation,
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
receipts. The exact run ID must commit before payload retirement. A crash after
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
owned bridge exits, restoring the same pre-Send handle is unavailable on this
pin, so recovery fails closed. No automatic Retry is implemented.

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
provenance while allowing the same conversation's work to advance. Unknown SDK
shapes fail closed. Live/paid Cursor behavior, native Keychain and packaged runtime
acceptance remain separate release gates; local fixture success proves wiring only.

Send diagnostic controls mirror creation: cloud-fixture.json `send_crash_at` or
`send_pause_at` accepts before_dispatch, after_claim, after_reply, after_ack;
pauses require an absolute `send_checkpoint` and end at its `.release` sibling
within 30 seconds. bridge.json `drop_send_reply` accepts then loses the reply;
`send_pause_file` stalls the stream until its `.release` sibling. Corruption
`send_scenario` values include cross_run, unknown_shape, bad_source, bad_call,
bad_success, missing_done, wrong_repository and wrong_snapshot. Tests use a
private Git-backed gh executable to exercise the same production link verifier
without network or real credentials; fixture evidence does not prove live GitHub.

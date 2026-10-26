# Cursor cloud validation and durable handle preparation

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
handle through the pinned SDK. This introduces no prompt entry or Send. The
acknowledgement is a local handle, not proof that a backend agent/run exists;
GetAgent is deliberately not required before a future first Send. Terminal
responses, Retry and active reattachment remain blocked.

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
also uses CreateAgent. The bridge receives its selected
key in a separate environment without OVRCR reporting or delegation credentials.

## Retention and local controls

The existing Server/SessionStore owns creation, using the same SQLite connection
as retained Sessions. Schema 21 retains the original Session/run, invocation,
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
ID and explicit model, then commits before retiring creation bytes. Future Send
must have its own independent intent, claim and exact run-receipt boundary.

An unresolved creation stays on its original row through exit and restart without
inventing a conversation reference, replacing its key, using a resume picker or
automatically creating another session. Archive preserves it. Explicit
`ovrcr terminal cloud-discard ID` closes local delivery and retires payloads;
it does not claim remote cancellation or resolve an escaped effect. Local row
deletion refuses unresolved creation until that Discard succeeds. An authenticated
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
The fixture accepts one exact creation shape and always rejects Send. It records
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

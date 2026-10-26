# Cursor cloud validation preview

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
preview and its owned bridge. There is no prompt entry, CreateAgent or Send in
this slice. Terminal responses, Retry and active reattachment remain blocked.

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
Me, ListModels and ListRepositories are used. The bridge receives its selected
key in a separate environment without OVRCR reporting or delegation credentials.

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
The fixture rejects noncatalog methods and records method names, process IDs and
resource paths, allowing zero CreateAgent/Send assertions and owned cleanup checks.
This proves native wiring and lifecycle with a local service, not real Keychain,
Cursor access, or official-bridge runtime behavior. Production pins remain strict.

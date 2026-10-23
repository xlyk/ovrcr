# Cursor cloud preflight (#363)

This standalone Python 3.11+ client implements only the preflight slice of
[#362](https://github.com/xlyk/ovrcr/issues/362). It calls no CreateAgent or Send,
changes no OVRCR runtime/provider, and makes no coding, recovery, cancellation or
native-provider feasibility claim. Later slices #364–#367 own those outcomes.
The official Cursor SDK Bridge is separate from OVRCR's macOS Bridge.

## Invocation

Live catalog calls require the exact account/repository/model/budget/effects
manifest to be approved. Implementation/offline approval does **not** authorize
real credential use, cloud calls, fixture publication or paid execution.

1. Copy `manifest.example.json` outside the source tree. Replace every placeholder
   with the approved nonsecret values. Use an exact disposable GitHub repository
   URL, matching owner/repository policy, full lowercase 40-character published
   SHA and exact available model. This slice validates the declaration; independent
   remote source/fixture verification belongs to #364/#367.
2. Obtain the standalone archive for the host from the explicit
   [v1.0.40 release](https://github.com/cursor/sdk-bridge/releases/tag/v1.0.40).
   The client never downloads, selects latest, or uses an installed bridge.
3. Supply the selected key in the environment variable explicitly named by
   `credential_ref`. Resolve it with an approved secret manager at execution;
   never put the key in the manifest, shell command arguments, source or logs.
   An existing `CURSOR_API_KEY` or IDE login is never a fallback.
4. After that separate authorization, invoke:

   ```sh
   rtk proxy python3 -I experiments/cursor-cloud/client.py \
     --manifest /absolute/path/approved-manifest.json \
     --archive /absolute/path/cursor-sdk-bridge-standalone-darwin-arm64.tar.gz
   ```

JSON is emitted on stdout, with no raw stderr/discovery/HTTP captures. Save it only
to the approved evidence destination. Exit 0 requires qualified preflight and
confirmed owned-process/token cleanup; exit 1 means preflight or cleanup failed.
Me is recorded as selected-key credential context, not stable billing identity.
The report records the requested USD ceiling; a local timer and unavailable usage
are **not** a remote spending cap or evidence of zero cost.

The configured future experiment ceiling is exactly one CreateAgent attempt and
three Send attempts, with no retries. Allowed effects are restricted to the fixture
change, result branch, exact-run cancellation and owned local shutdown. This slice
performs only local shutdown and catalog reads; the remaining effects stay NOT RUN.

## Pin and wire contract

The constants in `client.py` pin SDK **1.0.40**, bridge **1.0.0**, `sdk.v1`, and
standalone architecture-specific archive SHA-256 values. These were inspected from
GitHub's release asset digests and the darwin-arm64 archive on 2026-10-23; the
archive itself was not executed. Its manifest includes `runtime=bun-1.3.9` and
`bridgeCommit=863444c1b502763bc7e22777084d20d352d5cdf4`.

| Host | Archive SHA-256 |
| --- | --- |
| darwin-arm64 | `56f7f68a43ce73daa8d93c60b35b5301feb6a351e1dabfefe9873d80d56931cb` |
| darwin-x64 | `cc1e1779e9768fc0db09a93d98dce51498e507ee26a24ae04c0dd21b21700ea6` |
| linux-arm64 | `83fee2aa4ff753756c1993937e154961a69048620e66969ad36dfdd3f8439254` |
| linux-x64 | `2939798aa62e201417d9e1bba1a373ccc1d8da65fc3d95db386204c27379ac80` |

Archive manifest fields and runtime GetVersion are checked before catalog calls.
Requests use JSON unary POSTs at `/sdk.v1.SdkBridgeControlService/*` and
`/sdk.v1.SdkCursorService/*`, with explicit `options.apiKey` on catalog calls.
Ping with a bad bearer must return HTTP 401 / `unauthenticated`.
Primary contracts: [protocol/handshake](https://github.com/cursor/sdk-bridge/blob/1ba4fe546ea597c2f14b571b0ebe59ab1d20b70e/docs/protocol.md),
[control RPCs](https://github.com/cursor/sdk-bridge/blob/1ba4fe546ea597c2f14b571b0ebe59ab1d20b70e/proto/sdk/v1/sdk_bridge_control_service.proto),
[catalog RPCs](https://github.com/cursor/sdk-bridge/blob/1ba4fe546ea597c2f14b571b0ebe59ab1d20b70e/proto/sdk/v1/sdk_cursor_service.proto).

## Bounds and resource receipts

Absolute monotonic deadlines start on entry: work ends at 14 minutes and total
cleanup at 15 minutes. Unix `setitimer` also interrupts blocking process startup,
connect/write, HTTP headers, body and error reads; socket timeouts use the same
remaining budget. Bytes/keepalives do not renew it. The CLI must run on the main
thread of its standalone process; supported hosts are macOS/Linux arm64/x64.

Normal and error paths share one cleanup routine ending at
`min(cleanup entry + 60 seconds, original total deadline)`. Its final available
10 seconds are reserved for bounded TERM/wait/KILL/wait of the owned bridge PID.
A stalled Shutdown cannot consume that interval. Unconfirmed exit, including an interrupted launch before a process handle is returned,
remains UNRESOLVED and prevents private-root/token-file removal.

Each invocation creates a private root containing HOME, workspace, state and TMP.
The official token must be a mode-0600 regular, singly linked file named
`auth-token`, in a mode-0700 owned `cursor-sdk-bridge-*` directory immediately
inside that private TMP. It is read into memory without logging. After confirmed
bridge exit, cleanup checks the saved device/inode identities, removes only that
file and its empty directory, and separately records actual removal. Foreign,
replaced, nonprivate or unconfirmed token ownership is retained as UNRESOLVED.
Only known files and empty private directories are removed; unexpected state is
retained at the reported exact root. Never treat process exit as token deletion.

## Offline checks

```sh
rtk proxy python3 -I -m unittest discover -s experiments/cursor-cloud -p test_client.py -v
rtk proxy python3 -I -m py_compile experiments/cursor-cloud/client.py experiments/cursor-cloud/test_client.py experiments/cursor-cloud/fake_bridge.py
```

`test_client.py` invokes normal `main` in a fresh isolated Python process. Only the
external release-pin authority and deadline durations are replaced with synthetic
archive digests and short budgets before entry. There is no test-mode CLI flag,
endpoint attachment, alternate client constructor or production pin bypass.
`fake_bridge.py` is solely a synthetic external executable packaged by the test;
it runs real loopback HTTP and private token files and records method names and
boolean secret checks, never credential bodies. Synthetic sentinels are excluded
from all client output. Every fixture asserts zero CreateAgent/Send traffic and a
bounded return. Tests remove only their own fixture roots after recording any
client-reported uncertainty. CI runs this full feature suite on macOS and Linux.

| Acceptance | Entry-point assertion |
| --- | --- |
| Required explicit inputs / no ambient fallback | `test_missing_inputs*`, `test_missing_or_bad_manifest*`, `test_selected_key*` |
| Pinned checksum / architecture / archive manifest | `test_checksum_mismatch*`, `test_archive_manifest_mismatch*`, success receipt |
| Ready handshake, sdk.v1, explicit selected key/model/repository | `test_success*`, `test_runtime_auth_model_and_repository_mismatch*` |
| Startup / partial headers / body / error / trickle bounds | `test_stalls_and_trickle*` |
| Original total deadline applies to cleanup | `test_cleanup_is_truncated*` |
| Non-object discovery produces a redacted failure report | `test_malformed_discovery*` |
| Interrupted launch without a handle retains uncertainty | `test_interrupted_launch*` |
| Stalled shutdown reserves forced exit and token cleanup | `test_stalled_shutdown*` |
| Exact private token removal / changed or foreign identity retained | `test_success*`, `test_unknown_or_nonprivate_token*`, `test_replaced_token*` |
| Secret exclusion, unrelated-file preservation, zero cloud effects | common assertions in every fixture; success also checks argv and explicit environment |

These tests prove offline client behavior, not official-bridge runtime compatibility,
account permissions, cloud source selection, coding/control behavior, remote cost,
or native-provider readiness. The separately approved live proof remains #367.

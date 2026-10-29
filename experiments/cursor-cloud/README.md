# Cursor cloud preflight and one coding run (#363–#364)

This standalone Python 3.11+ client runs preflight and, with an explicit fresh
journal, one cloud creation and one coding Send for
[#362](https://github.com/xlyk/ovrcr/issues/362). `--preflight-only` retains the
catalog-only path. It changes no OVRCR provider. Observation, follow-up, cancellation
proof, fresh-bridge recovery and live acceptance remain NOT RUN (#365–#367).
Error cleanup can request cancellation of an exact known active run; that does not
count as the later cancellation proof. The official Cursor SDK Bridge is separate
from OVRCR's macOS Bridge.

## Invocation

Live catalog calls require the exact account/repository/model/budget/effects
manifest to be approved. Implementation/offline approval does **not** authorize
real credential use, cloud calls, fixture publication or paid execution.

1. Copy `manifest.example.json` outside the source tree. Replace every placeholder
   with the approved nonsecret values. Use an exact disposable GitHub repository
   URL, matching owner/repository policy, full lowercase 40-character published
   SHA and exact available model. Use the reviewed three-file fixture below. The coding path independently fetches
   the selected SHA and remote HEAD before any creation; preflight-only checks the
   declaration and catalog availability.
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
     --archive /absolute/path/cursor-sdk-bridge-standalone-darwin-arm64.tar.gz \
     --journal /absolute/path/new-private-experiment \
     --github-config /absolute/path/existing-gh-config \
     --github-login APPROVED_GITHUB_LOGIN
   ```

Add `--preflight-only` and omit `--journal` for catalog checks with zero
CreateAgent/Send calls. `--help` describes these paths. Coding requires Git at `/usr/bin/git` and an installed `gh` on the initial PATH.
The nonsecret GitHub login/config prerequisite is separate from the selected Cursor
key. `gh auth status --active --hostname github.com` must identify the approved
GitHub login in that config before execution. The client checks only projected
login/active/state metadata (no `--show-token`). It clears ambient GH_TOKEN and
GITHUB_TOKEN along with the rest of the inherited environment. Each fetch uses an
isolated HOME, disables system/global Git configuration and hooks, refuses prompts,
and supplies only a per-command `gh auth git-credential` helper scoped to the exact
approved repository URL, with useHttpPath, selected GH_CONFIG_DIR and username.
An unrelated repository on github.com receives no helper credential. The helper passes its credential directly to Git;
the client captures no token, writes no auth config and changes no global Git
settings. This supports an authorized private disposable repository. The client
never uploads local source.

JSON is emitted on stdout, with no raw stderr/discovery/HTTP captures. Save it only
to the approved evidence destination. Exit 0 requires successful preflight, the requested coding path (unless
preflight-only), confirmed owned-process/token cleanup, and no recorded failed
required stage. Exit 1 retains the failing stage and unresolved outcomes, including
failures caught after independently qualified coding-result evidence.
Me is recorded as selected-key credential context, not stable billing identity.
The report records the requested USD ceiling; a local timer and unavailable usage
are **not** a remote spending cap or evidence of zero cost.

The configured future experiment ceiling is exactly one CreateAgent attempt and
three Send attempts, with no retries. Allowed effects are restricted to the fixture
change, result branch, exact-run cancellation and owned local shutdown. This slice uses at most one creation and one Send, with separately generated
original keys. It never retries either operation or starts a replacement conversation.

## Fixture and authorized publication recipe

`fixture/` contains exactly `addition.py`, `test_addition.py` and `SOURCE_MARKER`.
The selected baseline subtracts: its three unittest cases produce exactly two
failures. This failing source is intentional. Only `addition.py` may change in the
result, including every intermediate commit after the selected source.

After the exact disposable repository and publication effects are approved:

1. In a new task-owned repository, copy only those three fixture files. Commit them
   and record the full lowercase SHA as the manifest's `source_sha`.
2. Preserve that commit on a published source ref. Its marker must remain exactly
   `OVRCR_SELECTED_BASELINE_364` followed by a newline.
3. On the repository's default branch, make one further commit changing only the
   marker to `OVRCR_DEFAULT_TIP_364` followed by a newline. Publish that default
   branch and verify the remote default HEAD and marker independently.
4. Keep the baseline addition and test files byte-for-byte equal to `fixture/`.
   Add no workflows, secrets, hooks, production files or deployment setup. Record
   the published selected/default SHAs with the approved manifest.

This is a recipe; this implementation publishes no repository. Offline tests make
all of these commits in their own local repositories, with an external Git peer
rewriting only the synthetic fixture URL to its owned local repository. That peer
first exercises real Git credential-fill against the selected synthetic `gh`
helper, so local URL rewriting cannot bypass authentication assertions.

## Admission, source and result evidence

The explicit journal is a mode-0700 directory with mode-0600 regular files. An
exclusive process lock covers the entire experiment. A retained journal refuses a
new invocation; choose neither a fresh journal nor a replacement operation to
retry an uncertain prior attempt. There is no replay command.

Before each effect, `<method>.request.json` contains the exact nonsecret request
and original operation key. The client fsyncs the request and directory, then
fsyncs its attempt claim in `events.jsonl`, before dispatch. Credentials enter only
the in-memory outgoing request. Claims retain the original key and request digest.
Secrets in JSON property names and nested values are rejected before publication.
Creation identity/model must be durably saved before GetAgent and the separately
keyed Send. GetAgent must match the exact conversation, cloud runtime and repository.
The early Send init supplies the exact run/agent/model receipt. Only after its
receipt is durable is the original request removed and that removal synced.
Missing receipts retain the original request and close admission. Journal capacity
is 256 KiB with closure reserve; storage failure blocks new effects and never
evicts an unresolved record. Owner-only plaintext remains accessible to same-user
programs and backups.

The prompt requires the first tool to execute exactly
`git rev-parse HEAD && cat SOURCE_MARKER`, then fix addition, run the fixture tests
and remember a random harmless public continuity token. Source qualifies only from
a correlated running/completed `shell` tool receipt, before any other tool. Its
`args.command` and `result` (`stdout`, `stderr`, integer `exitCode`) must match the
probe and independently fetched selected SHA/marker exactly. Assistant prose and
branch ancestry cannot qualify it. The
[SDK tool payloads are unstable](https://cursor.com/docs/sdk/typescript#stream-events):
unknown shapes remain unverified instead of being guessed.

Coding success also requires exact agent/run/model, FINISHED result, matching done
and a successful Connect end-stream frame. Keepalives and unknown envelope cases
are ignored; malformed, compressed, oversized, duplicate or out-of-order frames
fail. Git then independently fetches the returned result branch, checks ancestry,
checks the final diff and every intervening commit for only `addition.py`, and
runs the unchanged three tests in an isolated checkout with explicit `-I -B`
(isolated imports/environment and no stale bytecode). A failed test is a failure.

On the running client PID, SIGUSR1 records Cancel closure; SIGUSR2 records Discard
closure and removes retained request payloads. Both share the same local authority
as attempt claims. Closure before a claim blocks dispatch; a prior claim may have
escaped and remains uncertain until its exact receipt is durable. Discard is no
remote cancellation or secure-erasure claim. Late receipts are retained as late
and cannot recreate payloads, reopen admission or deliver a coding pass. The final
admission check, fsynced verified receipt and coding PASS decision share this same
authority. A local fence is separate from confirmed durable closure: failed closure
publication reports UNCONFIRMED. The overall invocation fails if required journal
persistence or closure fails, even when independent coding-result evidence passed.
Cleanup
requests cancellation only for an exact known active run, with durable intent;
an empty acknowledgement alone leaves the remote outcome unresolved. One bounded
GetRun reads its terminal state, without selecting or listing another run.

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
remaining budget. Bytes/keepalives do not renew it. Each blocking phase disarms
its timer before reporting; an already-expired phase fails synchronously. The CLI
must run on the main
thread of its standalone process; supported hosts are macOS/Linux arm64/x64.

Normal and error paths share one cleanup routine ending at
`min(first cleanup entry + 60 seconds, original total deadline)`. A failing
verification command anchors that same end while it still owns the exact child
handle and waits for actual exit confirmation within the pre-shutdown interval.
An interrupted close, owned-child termination, or exit observation can repeat
only while that same bound has time; it neither relaunches work nor renews the
clock. The short owned-child poll/signal transition defers SIGALRM so its one
termination attempt cannot be skipped between recording intent and signaling. Main cleanup reuses the end; it cannot start another 60-second window. Its final
available 10 seconds are reserved for bounded TERM/wait/KILL/wait of the owned
bridge PID. A stalled child wait or Shutdown cannot consume that interval.
Unconfirmed bridge exit, including a launch interrupted before a handle returns,
remains UNRESOLVED and prevents private-root/token-file removal. An unconfirmed
verification child retains its receipt and private verification root even when
bridge exit and token removal are separately confirmed.

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

`test_client.py` invokes normal `main` in a fresh isolated Python process. The external release-pin authority and deadline durations are replaced with
synthetic archive digests and bounded test budgets before entry. Preflight keeps
its 3-second work / 5-second total budget. Normal coding uses 15/20 seconds because
its source/default/result fetches, credential-helper checks and independent tests
run sequential external processes against one work clock. These budgets let semantic cases reach their
asserted admission/result boundary. A controlled latency regression
adds an observed 250 ms wait inside each real Git peer process: normal coding must
finish all three fetches and both fixture test results, while an explicit 2-second
work clock must still stop before creation. No clock is renewed. Stall peers record
that the intended phase was reached and trickle beyond the absolute deadline. Expired-entry
and exhausted-cleanup cases keep separate zero/late budget checks. New coding
stall cases use the named normal coding work/total allowance so preparation can
reach each fault. They still require an actual peer or process phase receipt,
the exact failure stage, the absolute deadline and observed child exit. Their
1.5-second cleanup limit and 0.75-second shutdown reserve remain explicit; other
fault methods retain their own short clocks. Production limits stay 840/900 seconds. Cancellation cleanup retains its explicit 0.4-second budget and 0.15-second
shutdown reserve. Source-admission failures must match their expected reason, so a
deadline in the same stage cannot qualify that guard. Coding cases substitute
the external Git executable with an owned URL-rewriting peer; Git itself and all
repositories, files, processes and sockets are real. Narrow test-runner tracing
forces real signal/storage faults at final verification and cleanup boundaries;
it does not replace the client's authority, constructors or persistence. There is
no test-mode CLI flag,
endpoint attachment, alternate client constructor or production pin bypass.
`fake_bridge.py` is solely a synthetic external executable packaged by the test;
it runs real loopback HTTP and private token files and records method names and
boolean secret checks, never credential bodies. Its loopback server binds directly
without reverse DNS; controlled stalled/failed resolver cases prove startup has no
external name-resolution dependency. Synthetic sentinels are excluded
from all client output. Every preflight fixture asserts zero CreateAgent/Send traffic. Coding fixtures
assert exact counts/order, write-ahead bytes and modes at the external peer,
retained identities, expected rejection stages and a bounded return. The fixture
records actual spawned process handles and stops survivors only after normal main
returns its report; fixture cleanup never qualifies client exit or token removal.
Tests remove only their own roots after recording
client-reported uncertainty. CI runs this full feature suite on macOS and Linux.

| Acceptance | Entry-point assertion |
| --- | --- |
| Required explicit inputs / no ambient fallback | `test_missing_inputs*`, `test_missing_or_bad_manifest*`, `test_selected_key*` |
| Pinned checksum / architecture / archive manifest | `test_checksum_mismatch*`, `test_archive_manifest_mismatch*`, success receipt |
| Ready handshake, sdk.v1, explicit selected key/model/repository | `test_success*`, `test_runtime_auth_model_and_repository_mismatch*` |
| Startup / partial headers / body / error / trickle bounds | `test_stalls_and_trickle*` |
| Synthetic loopback startup independent of stalled/failed reverse DNS | `test_loopback_peer_startup*` |
| Expired entry reports; exhausted cleanup retains uncertainty | `test_expired_entry*`, `test_expired_cleanup*` |
| Original total deadline applies to cleanup | `test_cleanup_is_truncated*` |
| Non-object discovery produces a redacted failure report | `test_malformed_discovery*` |
| Interrupted launch without a handle retains uncertainty | `test_interrupted_launch*` |
| Stalled shutdown reserves forced exit and token cleanup | `test_stalled_shutdown*` |
| Exact private token removal / changed or foreign identity retained | `test_success*`, `test_unknown_or_nonprivate_token*`, `test_replaced_token*` |
| Secret exclusion, unrelated-file preservation, zero cloud effects in preflight | common assertions in every fixture; success also checks argv and explicit environment |

The #364 acceptance cases map to these additional entry-point assertions:

| #364 criterion | Assertion |
| --- | --- |
| 1: existing path, explicit cloud options/SHA/model, one creation/Send | `test_coding_verifies_patch_and_exact_write_ahead_order` plus peer exact request checks |
| 2: falsifiable fixture, selected/default source distinction and private-fetch auth | `test_independent_fixture_source_admission_precedes_any_creation`, `test_selected_github_helper*` and success baseline/result test receipts |
| 3: private lock, durable claims, bounded capacity | `test_private_owner_storage_and_capacity_fail_before_new_effect` and external write-ahead checks |
| 4: durable exact creation before GetAgent/Send | success, `test_lost_receipts*`, creation/GetAgent identity/model cases in `test_coding_rejects*` |
| 5: exact early run, source probe, framing/result/end | `test_coding_rejects*`; success includes split frames, keepalives and unknown envelopes |
| 6: source, ancestry, allowed history/diff, independent tests | success and wrong-source/ancestry/diff/failed-tests cases in `test_coding_rejects*` |
| 7: uncertain receipt retention; no replacement/retry/guessed run | `test_lost_receipts*`, storage/identity cases and common method-count assertions |
| 8: closure ordering, late receipts, exact cleanup cancel | `test_closure_orders*`, `test_final_verified*`, `test_failed_required_closure*`, `test_only_exact_known_active_run*` |
| 9: absolute partial-frame/Git/test/cancel deadlines, honest report | `test_partial_frames*`, `test_known_verification_child_reaping*`, `test_stale_alarm_retries*`, `test_slow_external_processes*`, `test_caught_deadline_after_coding_pass*`, exact-cancel stalled case and inherited preflight/cleanup deadline tests |
| Property-name/value secret exclusion | `test_json_property_names*` and common stdout/journal sentinel checks |
| 10: actual entry/storage/Git/process path and failure matrix | the full `test_client.py` suite; runtime criterion 10 stays NOT RUN because one invocation cannot attest the suite |

The JSON report includes stage errors, criterion-specific receipts, original keys,
identities, source/result checks, deadline configuration, verification-process waits
and resource disposition. Later proof stages and missing cost remain explicitly
NOT RUN or unavailable.

These tests prove offline client behavior, not official-bridge runtime compatibility,
account permissions, cloud source selection, coding/control behavior, remote cost,
or native-provider readiness. The separately approved live proof remains #367.

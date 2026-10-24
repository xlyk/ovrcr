# Cursor cloud three-run proof (#363–#366)

This standalone Python 3.11+ client runs preflight and, with an explicit fresh
journal, one cloud creation and three sequential Sends for
[#362](https://github.com/xlyk/ovrcr/issues/362). `--preflight-only` retains the
catalog-only path. Run 1 exercises exact-run observation recovery, then Run 2 asks
for the earlier public token and fix in the same conversation. Run 3 requests a
harmless wait of at most 90 seconds and proves exact-run cancellation. Once all
three runs are terminal, an optional owned bridge restart retrieves the exact
completed Run 1 read-only. It changes no OVRCR provider. Cloud lifecycle acceptance remains
unverified (#367). Error cleanup of an earlier run is separate from Run 3's proof. The official Cursor SDK Bridge is separate
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
3. Supply the selected Cursor key in the environment variable explicitly named
   by `credential_ref`. Coding also requires `github_credential_ref` to name
   `env:OVRCR_POC_GITHUB_PAT` (or another explicitly approved variable) containing
   the selected GitHub PAT. Resolve both with the approved secret manager at
   execution; never put values in the manifest, arguments, source, logs or configs.
   Missing/empty/invalid PAT, aliased credential references or equal credential
   values stop before bridge startup. Ambient
   `GH_TOKEN`, `GITHUB_TOKEN`, a keyring login or `CURSOR_API_KEY` is no fallback.
4. After that separate authorization, invoke:

   ```sh
   rtk proxy python3 -I experiments/cursor-cloud/client.py \
     --manifest /absolute/path/approved-manifest.json \
     --archive /absolute/path/cursor-sdk-bridge-standalone-darwin-arm64.tar.gz \
     --journal /absolute/path/new-private-experiment \
     --github-login APPROVED_GITHUB_LOGIN
   ```

Add `--preflight-only` and omit `--journal` for catalog checks with zero
CreateAgent/Send calls. That path does not resolve a GitHub PAT and may omit
`github_credential_ref`. `--help` describes these paths. Coding requires Git at
`/usr/bin/git`, an installed `gh` on the initial PATH and an exact
`--github-login` matching the PAT. The selected GitHub identity is separate from
the Cursor key.

The client supplies only the explicitly selected PAT as `GH_TOKEN` to trusted
`gh auth status`, PR queries and the Git remote process that invokes the scoped
credential helper. A new private `GH_CONFIG_DIR` is created inside the owned
runtime root; no user GitHub config or keyring supplies a fallback. Projected
metadata must show the exact login, active status, success and
`tokenSource=GH_TOKEN`. A successful command exit alone is insufficient.
`--github-config` was intentionally removed: existing coding invocations must add
`github_credential_ref`, select the PAT and keep `--github-login`.

Each fetch retains isolated HOME, disabled system/global Git configuration and
hooks, no prompts, and only a per-command `gh auth git-credential` helper scoped
to the exact approved repository URL with useHttpPath and the selected username.
An unrelated repository receives no helper credential. The helper passes its
credential directly to Git; the client captures no token, writes no auth config
and changes no global settings. The bridge receives only its Cursor key; local
Git and fixture/test processes receive neither credential. Both selected secrets
are rejected from private journal keys/values and redacted from final reports.
The client never uploads local source.

JSON is emitted on stdout, with no raw stderr/discovery/HTTP captures. Save it only
to the approved evidence destination. Exit 0 requires successful preflight, the requested coding path (unless
preflight-only), confirmed owned-process/token cleanup, and no recorded failed
required stage. Exit 1 retains the failing stage and unresolved outcomes, including
failures caught after independently qualified coding-result evidence.
Me is recorded as selected-key credential context, not stable billing identity.
The report records the requested USD ceiling; a local timer and unavailable usage
are **not** a remote spending cap or evidence of zero cost.

The experiment ceiling is exactly one CreateAgent attempt and three Send attempts,
with no retries. Allowed effects are restricted to the fixture change, result branch,
exact-run cancellation and owned local shutdown. Every Send uses a separately
generated original key. Run 2 follows independently qualified Run 1; Run 3 follows
qualified Run 2. Uncertain receipts never buy another Send or conversation.

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
The pinned SDK's CreateAgent allocates a local cloud handle; the first Send
creates the backend agent and run together. The allocated agent ID/model must be
durably saved before the separately keyed Send. The first recognized SDK message
supplies the exact run/agent receipt; cloud streams do not emit `system/init`. Only
after that receipt is durable is the original request removed and that removal synced. GetAgent then must match the exact conversation,
cloud runtime and repository before observation or tool-event processing. A failed
metadata check blocks coding/follow-up qualification and leaves the exact known run
available for bounded error cleanup. Criterion 4 requires validated metadata, not
merely a Send attempt. Closed admission still retains late Send/result receipts
without dispatching a metadata lookup or new work.
Missing receipts retain the original request and close admission. Journal capacity
is 256 KiB with closure reserve; storage failure blocks new effects and never
evicts an unresolved record. Owner-only plaintext remains accessible to same-user
programs and backups.

The prompt requires the first tool to execute exactly
`git rev-parse HEAD && cat SOURCE_MARKER`, then fix addition, run the fixture tests
and remember a random harmless public continuity token. Source qualifies only from
a correlated running/completed `shell` or `run_terminal_cmd` tool receipt, before
any other tool. Its `args.command` must be the probe, optionally prefixed with
`cd /workspace && ` as observed in Cursor; execution metadata does not change that
command check. Cloud `run_terminal_cmd` can first announce the call without args,
then supply them in a second running update. All updates must retain the same
agent/run/call/name; completion must supply the exact command. Its captured
`result.success` must be foreground, with stdout and interleaved output both equal
to the independently fetched SHA/marker. That cloud shape omits exitCode/stderr;
any explicit nonzero exit, stderr or signal still fails. The `shell` form keeps
its existing flat or `{"status": "success", "value": ...}` check, including empty
stderr/signal and integer zero exitCode.

`source-tool-capture.json` preserves the three proof-5 tool events with synthetic
agent/run/call/request IDs and a source-SHA placeholder. The entry-point regression
replays it through the existing fake bridge; no live calls or credentials are used.
Redacted source-tool events are
saved in the existing private bounded journal before validation, including rejected
events; no raw credentials or discovery data are captured. Assistant prose and
branch ancestry cannot qualify it. The
[SDK tool payloads are unstable](https://cursor.com/docs/sdk/typescript#stream-events):
unknown shapes remain unverified instead of being guessed.

Coding success also requires exact agent/run identity, FINISHED result, matching
done and a successful Connect end-stream frame. The selected model is explicit in
CreateAgent and every Send. When the initial message or terminal result supplies a
model, it must match; an omitted model is not an attestation. The pinned bridge
reconstructs cloud handles on GetRun without their original requested model, so
ObserveRun results may omit it. Keepalives and unknown envelope cases
are ignored; malformed, compressed, oversized, duplicate or out-of-order frames
fail. Git then independently fetches the returned result branch, checks ancestry,
checks the final diff and every intervening commit for only `addition.py`, and
runs the unchanged three tests in an isolated checkout with explicit `-I -B`
(isolated imports/environment and no stale bytecode). A failed test is a failure.

After Run 1's early exact identity is durable, an exact GetRun snapshot must show
RUNNING or successful FINISHED status. ERROR, CANCELLED and EXPIRED snapshots are
durably retained and immediately stop qualification and follow-up, including during
attachment; a later contradictory FINISHED stream cannot override them. The client closes the original Send, then calls
ObserveRun with only that runId, from the beginning. Previously processed events
are labelled replay and de-duplicated. Each qualifying event still matches the
exact agent/run. The client records offsets only from ObserveRun, then, when an
active snapshot and a durable offset are available, closes that observer and
makes one ObserveRun call with that exclusive afterOffset. A repeated cursor is
rejected. Missing or unexercised offsets remain NOT RUN; a terminal snapshot before
the original interruption qualifies only completed-result recovery. No CreateAgent,
Send or CancelRun is dispatched as part of observation. Failure closes admission;
only the existing exact-run error cleanup may cancel a known active run.

Run 2 uses the same agent, journal and global counters, a new original operation
key and a distinct durable run ID. Its prompt never restates the public token and
asks for the token and completed fix without file, branch or PR changes. Success
requires exact terminal identity and stream end (and matching model when supplied),
the exact recalled token/fix, unchanged result repository/branch/PR metadata, identical independently read
advertised Git refs (including tags and the verified Run 1 commit), and identical
independent GitHub PR snapshots. It does not re-edit or rerun
the fixture: the previously tested commit must remain unchanged. Unknown result
shapes cannot qualify. A lost Run 2 receipt retains its original request and never
cancels Run 1 or dispatches a replacement.

Run 3 asks only for `sleep 90`, forbidding edits, branch publication and PRs. Its
early original-stream identity must be distinct from both earlier runs and durable
before a running tool/status event qualifies activity. The client then closes
admission durably, records exact cancellation intent, and calls CancelRun once for
that agent/run. Selection of the first cancel closure and its attempt claim share
the existing signal authority. A durable discard winning before this claim permits
no CancelRun; an already claimed cancellation may still arrive. Empty acknowledgement is only requested. An exact terminal GetRun
snapshot or correlated terminal ObserveRun result must prove CANCELLED. FINISHED
winning the race is BLOCKED/inconclusive and never triggers a fourth Send. An exact
original terminal result arriving before activity is durably retained immediately;
no later snapshot or cleanup can replace it with uncertainty or recancel it. Missing,
wrong or reused identities retain uncertainty without discovery Send/ListRuns.
Late receipts cannot reopen admission, and an already attempted CancelRun is not
reissued during cleanup.

After all three known runs are terminal, the client uses the same cleanup routine
to stop its original bridge and account for the exact token. A replacement bridge
is admitted only after both process exit and token removal are confirmed. The
cached cleanup deadline is retained; the probe runs before its shutdown reserve
and before the original work deadline. Fresh GetRun supplies the exact completed
Run 1 ID and explicit cloud/agent/key routing options. Those are read options, not
remote AgentOptions updates. Unavailable/invalid retrieval stays BLOCKED with no
new work. The pinned ResumeAgent method explicitly applies updated options, so it
is never called: option-preserving ResumeAgent and active continuation after
process death remain unverified. Completed-run retrieval proves neither capability.

One bounded GetUsage read may record available usage on the original known bridge
before shutdown, independently of fresh-bridge retrieval. Costs retain
the official float-cent fields: `chargedCents` is charged cost, `rawCostCents` is
undiscounted cost. Optional absent cost remains UNAVAILABLE, and eventually
consistent billing is no remote spending-cap proof. Usage errors never trigger
polling, an attachment or new work.

Independent PR checks use `gh pr list` for the exact disposable repository with
`--state all` and the explicitly selected gh configuration. Each snapshot rechecks
the selected active login; ambient tokens are excluded. Compare PR identity/URL,
title/body, state/draft, head/base names and OIDs, update time, labels, assignees,
review requests, milestone, auto-merge and maintainer-change permission. The
snapshot is bounded to 100 PRs; reaching that limit fails closed as incomplete.
Unavailable, malformed, oversized or stalled reads cannot qualify follow-up.
Private journal receipts contain count, PR numbers and a canonical digest, without
copying PR bodies. Reads use the original absolute command/cleanup deadlines.

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
GetRun reads its terminal state, without selecting or listing another run. Each
attempt's report record retains its actual terminal outcome or unresolved identity.

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
[catalog RPCs](https://github.com/cursor/sdk-bridge/blob/1ba4fe546ea597c2f14b571b0ebe59ab1d20b70e/proto/sdk/v1/sdk_cursor_service.proto),
[agent RPCs](https://github.com/cursor/sdk-bridge/blob/1ba4fe546ea597c2f14b571b0ebe59ab1d20b70e/proto/sdk/v1/sdk_agent_service.proto),
[result/snapshot shapes](https://github.com/cursor/sdk-bridge/blob/1ba4fe546ea597c2f14b571b0ebe59ab1d20b70e/proto/sdk/v1/sdk_messages.proto),
and [stream/cursor semantics](https://github.com/cursor/sdk-bridge/blob/1ba4fe546ea597c2f14b571b0ebe59ab1d20b70e/docs/streaming.md).
The newly used contracts were read at this same pin on 2026-10-23; this is source
evidence, not official-runtime compatibility evidence.

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
retained at the reported exact root. This includes any nonempty private GitHub
config or HOME metadata such as a gh device identifier; it is reported as retained
for owned operator inspection rather than recursively deleted. Never treat
process exit as token deletion.

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
finish all three fetches, both fixture test results and both Run 2 all-ref reads,
plus independent PR verification, while an explicit 2-second
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
forces real signal/storage faults at final verification and closure boundaries.
The capacity fault targets the first actual Journal.close, including Run 3 cancel
closure, so an earlier successful closure cannot make the persistence test vacuous;
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

`test_source_probe_accepts_workspace_prefix_and_checks_sha_and_marker` exercises
the observed working-directory prefix through the normal entry point, accepting
the correct output and separately rejecting a wrong SHA or marker.

The stream fixture follows SDK 1.0.40's shipped `consumeSseStream` and
`sdkMessageToRunStreamMessage`: the outer `sdkMessage.type` wraps the complete SDK
message, including its own `type`, `agent_id` and `run_id`. It begins with thinking
or status rather than a synthetic init and omits model from observed results.
`test_cloud_stream_without_init_or_observed_model_reads_result` exercises coding,
observation, follow-up and cancellation through the normal client entry point.

The peer models lazy cloud creation: GetAgent returns 404 until the first Send.
`test_cloud_metadata_waits_for_first_send_materialization` checks durable allocation,
Send claim/identity, metadata, source probe and verified result in order. Wrong cloud
identity/runtime/repository or unavailable metadata after the run receipt must keep
criterion 4 BLOCKED, dispatch no follow-up, and clean up only the exact known run. The
close-before-Send fixture injects discard immediately after the durable CreateAgent
receipt, preserving its zero-Send boundary without a pre-Send metadata lookup.

The discard-after-Send peer receives the real claimed request, records durable
discard through the existing signal/journal gate, then emits the early identity and
terminal result. Both must be retained as late, with no observation or new work.
The runner controls the signal-masked Send receipt directory-sync boundary: the
old peer waited for closure while that signal was masked and could time out before
sending the late result. The corrected peer has already established closure before
that boundary. The original hosted scheduling delay remains unmeasured; this is a
controlled ordering proof, with no product-clock change or widened sleep.

| Acceptance | Entry-point assertion |
| --- | --- |
| Required explicit inputs / no ambient fallback | `test_missing_inputs*`, `test_missing_or_bad_manifest*`, `test_selected_key*` |
| Explicit GitHub PAT, source and private config | `test_coding_requires_explicit_github_pat*`, `test_selected_github_helper*`, `test_catalog_preflight*` |
| Credential process separation and redaction | `test_cursor_and_github_credentials*`, `test_selected_github_pat_reaches*`, `test_selected_github_pat_is_redacted*`; actual child environment and common stdout/journal assertions |
| Unknown private GitHub metadata retained safely | `test_private_github_config_residue*` |
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
| 4: durable allocated identity before Send; exact backend metadata after durable run identity | `test_cloud_metadata_waits_for_first_send_materialization`, `test_lost_receipts*`, creation/GetAgent identity/model and metadata-failure cleanup cases in `test_coding_rejects*` |
| 5: exact early run, source probe, framing/result/end | `test_coding_rejects*`; success includes split frames, keepalives and unknown envelopes |
| 6: source, ancestry, allowed history/diff, independent tests | success and wrong-source/ancestry/diff/failed-tests cases in `test_coding_rejects*` |
| 7: uncertain receipt retention; no replacement/retry/guessed run | `test_lost_receipts*`, storage/identity cases and common method-count assertions |
| 8: closure ordering, late receipts, exact cleanup cancel | `test_closure_orders*`, `test_final_verified*`, `test_failed_required_closure*`, `test_only_exact_known_active_run*` |
| 9: absolute partial-frame/Git/test/cancel deadlines, honest report | `test_partial_frames*`, `test_known_verification_child_reaping*`, `test_stale_alarm_retries*`, `test_slow_external_processes*`, `test_caught_deadline_after_coding_pass*`, exact-cancel stalled case and inherited preflight/cleanup deadline tests |
| Property-name/value secret exclusion | `test_json_property_names*` and common stdout/journal sentinel checks |
| 10: actual entry/storage/Git/process path and failure matrix | the full `test_client.py` suite; runtime criterion 10 stays NOT RUN because one invocation cannot attest the suite |

The #365 entry-point assertions add:

| #365 criterion | Assertion |
| --- | --- |
| 1–2: active Send drop, beginning replay, durable ObserveRun-only exclusive resume | `test_recovery_and_follow_up*`; peer confirms both real socket closes and checks the saved cursor before dispatch |
| 3: completion races and unexercised offset remain separate | `test_completion_race*`; inherited source/diff/tests/framing rejection cases still run through recovery |
| 4: same-agent distinct Run 2, separate original key, recall and unchanged Git state | `test_recovery_and_follow_up*`, `test_follow_up_rejects*`, `test_independent_follow_up_ref_and_pr*`, `test_pr_verification*` |
| 5: no replacement on error, retained Run 2 uncertainty, exact Run 2 cleanup | `test_observation_errors*`, `test_follow_up_uncertain*` and inherited closure/lost-receipt cases |
| 6–7: separate receipts/reasons, actual wire/private records and global bounds | `recovery_criteria` plus every new case's method counts, peer request-shape/write-ahead and journal assertions |

Inherited success expectations intentionally advance to three Sends, include
GetRun/ObserveRun/CancelRun and read-only restart traffic, and four original keys
(one creation plus three Sends). Selected-helper calls intentionally
increase from three to exactly five: source SHA, default HEAD, result branch, and
both Run 2 all-ref snapshots. PR API reads recheck the selected gh account separately. Rejection assertions still forbid
replacement work; source/framing cases now fail on the observation path where
applicable. Discard after Run 1 verification prevents Run 2, so a qualified coding
result can coexist with overall failure. Failed-terminal snapshot cases cover ERROR/CANCELLED/EXPIRED before drop and
during attachment via `test_failed_terminal_snapshots*`. Existing production clocks are unchanged.

The #366 entry-point assertions add:

| #366 criterion | Assertion |
| --- | --- |
| 1–2: final separately keyed Send, exact activity/intent/cancel and terminal proof | `test_third_send_cancels*`; peer verifies durable activity and closure before exact CancelRun |
| 3: acknowledgement, completion race, unknown/reused IDs and late receipts | `test_third_cancellation_ack*`, `test_third_terminal_before_activity*`, `test_third_discard_winning*`; no fourth Send or discovery target |
| 4–5: owned fresh read-only GetRun, unsupported option-preserving ResumeAgent | `test_fresh_read_only_probe*`; no ResumeAgent/options mutation or new work |
| 6: original deadlines/reserve, both bridge lifecycles and token leftovers | `test_third_stalled_control*`, `test_restart_waits*`, `test_fresh_read_only_probe*` and inherited cleanup cases |
| 7–8: per-run identities/outcomes, usage availability and native capability gaps | `test_third_send_cancels*`, `test_usage_cost*`, fault reports and common ownership assertions |
| 9: actual main/external process/Git/storage boundaries | full suite, peer method order and exact shapes, private journal and original wait receipts |

The JSON report includes stage errors, criterion-specific receipts, original keys,
identities, source/result checks, deadline configuration, verification-process waits
and resource disposition. `recovery_criteria` separately records active observation, completed-result recovery,
offset resume and follow-up. `cancellation` and `fresh_bridge` retain separate
verdicts/reasons; `runs` accounts for every Send. `resources.previous_bridge`
records the original owned exit/token disposition separately from the final bridge.
The original work/total/cached cleanup deadline values remain in `deadlines`.
`remaining_remote` retains the conversation/result branch, without remote deletion.
Terminal-response, uncertain-retry, ResumeAgent preservation and active process-death
continuation stay explicitly unresolved; missing cost remains unavailable.

These tests prove offline client behavior, not official-bridge runtime compatibility,
account permissions, cloud source selection, coding/control behavior, remote cost,
or native-provider readiness. The separately approved live proof remains #367.

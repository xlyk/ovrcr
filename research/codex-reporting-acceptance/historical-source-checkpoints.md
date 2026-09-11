# Codex source contract: blocked at certification

Current continuation, 2026-09-11 UTC: authorized credential reuse and CUA isolated hook trust succeeded. A trusted task-owned helper observed immediate parent PID matching the pre-exec native root; the exact reported file had a matching `session_meta` header with `history_mode=paginated` and a first-turn usage record. See [authenticated evidence](authenticated-attempt/checkpoint.md). ENOSPC stopped the matrix after this one root turn. Authentication is no longer the blocker. Production receiver-side peer-PID/parent authentication and paginated lineage/replay/remaining matrix evidence remain open; no adapter is certified or implemented. The original source/unauthenticated checkpoints below are retained as history, not current claims that authentication is unavailable.

Inspected 2026-09-10 PDT / 2026-09-11 UTC at OVRCR revision `a89d6a05648aa41a8ec2eab15f3e5b8be83c3932`, branch `codex/codex-reporting`, base `983dd42a05690b15db073e6c1d8454d5a889db63`. No runtime edits or native reporting support are delivered.

**Decision: no route selected.** One bounded source-comparison pass establishes candidates, not the safe exact-root binding required by Task 1. Dependent adapter work stops. This is a certification blocker, not a claim that Codex cannot expose a rollout file. The initial zero-attempt checkpoint was rejected in independent review. The corrected [native attempt](native-attempt/results.md) used the permitted matrix and one repeat, reached the actual login selector, and stopped without selecting authentication. No credentials, trust bypass, approval responder, live session query, or live configuration change occurred.

## Provenance

Installed `/opt/homebrew/bin/codex` resolves to `/opt/homebrew/Caskroom/codex/0.153.0/bin/codex`. Package metadata states version `0.153.0`, target `aarch64-apple-darwin`. Executable SHA-256: `a29d9e86eef88cbbd69f97ce8c590b1d0a287c8f77424f5eef226b883d7eaa22`.

Public tag `rust-v0.153.0`, annotated tag object `6bc50f104dcc0192e696cdeae721dfc19b507391`, resolves to [source revision 41e22fee](https://github.com/openai/codex/tree/41e22fee981a63b3698df7ed36bad393cda24715). The tag is unsigned. Installed version/package match that release label; binary reproducibility against source was not checked. Local generated schema takes precedence over rolling documentation for installed field availability.

Candidate schema was generated with `codex app-server generate-json-schema --experimental --out` in a `mktemp` directory. [Versioned fixtures](../../tests/fixtures/agent-reporting/codex/0.153.0/README.md) retain only allowlisted schema fragments and trailing-whitespace-normalized help, their hashes, and hashes of inspected public source files. They contain no native event recordings. Temporary public source/schema data is not a runtime dependency.

## Route comparison

| Route | Established | Missing primitive or proof | Decision |
| --- | --- | --- | --- |
| Passive native app-server stream | Installed request enum has `thread/read`, `thread/resume`, `thread/unsubscribe`; no `thread/subscribe`. Initialize capabilities have no observer role or thread selection. Read response implementation only reads the requested view. Resume lifecycle joins connection subscriptions. | An exact-thread passive subscription that leaves native approval routing and disconnect behavior unchanged. No such supported operation was found in this candidate comparison. | Not selected; `thread/resume` is prohibited for telemetry. |
| Metadata/history polling | Read accepts exact `threadId`; turns/items APIs expose pagination. | Trusted launch-to-root identity and owned endpoint; neither snapshot polling nor a separately launched server proves receipt of the native thread's live status/token stream. | Not selected; no server launched. |
| Versioned exact rollout | Root hook dispatch is distinct from child hook dispatch. Native hook path comes from the live thread's selected local rollout and requests materialization. | Actual invocation-to-root certification, trusted enabled hook delivery, matching header, versioned record/lineage semantics under paginated history, numeric native comparison. These were not demonstrated. | Real candidate, uncertified; not rejected solely for being unstable. |

[Official app-server documentation](https://learn.chatgpt.com/docs/app-server) says `thread/read` does not subscribe. Its approval protocol expects a client response. That does not certify safe passive observer routing. The installed resume schema says it rejoins a running thread; it can also load another thread from history/path. No resume request was sent.

[Official hook documentation](https://learn.chatgpt.com/docs/hooks) describes session identity, nullable transcript paths, separate child hooks, and an unstable transcript format. A versioned reader remains possible in principle; instability alone is not a rejection. Help has no native supervisor-selected session-ID option, but a selected UUID is not the only possible proof. The inspected `SessionStartRequest` carries session ID, cwd, nullable path, model, permission mode and target; it has no supervisor invocation nonce, native PID or ancestry field. Root-specific dispatch distinguishes native subagents; it does not by itself distinguish an independent nested `codex` root inheriting callback configuration from the selected root. A process-bound root-only callback channel could potentially supply that proof, but its inheritance/routing guarantee was not established in this pass. First arbitrary callback admission remains prohibited.

Pinned source details:

- [hook_runtime.rs lines 124–163](https://github.com/openai/codex/blob/41e22fee981a63b3698df7ed36bad393cda24715/codex-rs/core/src/hook_runtime.rs#L124): pending root startup dispatches `SessionStart`; thread-spawn child startup dispatches `SubagentStart` with agent identity; other subagent sources return without startup hooks. The request uses the session identity and native hook transcript path. This is source evidence for a discriminator, not an invocation-routing test.
- [session/mod.rs lines 4586–4604](https://github.com/openai/codex/blob/41e22fee981a63b3698df7ed36bad393cda24715/codex-rs/core/src/session/mod.rs#L4586): `hook_transcript_path` obtains `live_thread.local_rollout_path`, returns null when unavailable/error, and requests materialization before returning the path. No actual header was opened because no trusted task-owned native path was captured.
- [thread_processor.rs lines 2790–2807](https://github.com/openai/codex/blob/41e22fee981a63b3698df7ed36bad393cda24715/codex-rs/app-server/src/request_processors/thread_processor.rs#L2790): read builds the exact requested thread view. [thread_lifecycle.rs line 694](https://github.com/openai/codex/blob/41e22fee981a63b3698df7ed36bad393cda24715/codex-rs/app-server/src/request_processors/thread_lifecycle.rs#L694) adds the connection during pending resume handling.
- [thread_rollout_resolver.rs lines 67–108](https://github.com/openai/codex/blob/41e22fee981a63b3698df7ed36bad393cda24715/codex-rs/thread-store/src/local/thread_rollout_resolver.rs#L67): live writer then SQLite-selected path precede filesystem fallback. For paginated threads SQLite selection is authoritative because revert can leave older immutable rollouts with the same ID; unavailable selected paths return none. A matching UUID alone therefore cannot certify the current lineage. No directory scan or newest-file selection was performed.

The rolling app-server page says paginated creation/full reads are not supported, while installed schema and pinned source contain paginated reads/resume handling. This discrepancy is unresolved by documentation alone. Do not assume legacy JSONL semantics, a fixed file, or missing pagination support from that page.

## Candidate event/metric contract (all version 0.153.0; none live certified)

| Native event/source | Root identity | Turn identity | Endpoint/path and ordering | Candidate interpretation / limit |
| --- | --- | --- | --- | --- |
| `SessionStart` hook | `session_id`; root-specific dispatch in source | Not established by retained hook input | Native `transcript_path` nullable; startup/resume/clear/compact source | Admission candidate only after independent launch/root proof. |
| `SubagentStart` / `SubagentStop` hook | Parent session identity plus child agent identity | Event-specific turn ID | Hook channel; async ordering not certified | Never admit child as root; no root freshness from child completion. |
| `UserPromptSubmit`, `PreToolUse`, `PermissionRequest`, `PostToolUse`, `Stop`, `Interrupt` | Hook session identity; root correlation required | Event-specific `turn_id` per docs; native order untested | Hook delivery; concurrent background hooks may reorder | Busy/wait/idle/error mappings remain unverified; permission request alone does not prove visible unanswered selector. |
| `thread/status/changed` | `threadId` | **No turn ID in schema** | Native app-server notification; no public source sequence in payload | `notLoaded`, `idle`, `systemError`, `active`; active flags `waitingOnApproval`, `waitingOnUserInput`. Delayed status needs current-turn reconciliation before mapping. |
| `thread/tokenUsage/updated` | `threadId` | `turnId` | Native app-server notification; subscription unavailable | `total` and `last` are separate breakdowns; candidate conversation cumulative vs current context requires native semantics/reference. |
| Token breakdown | Same admitted thread | As above | `inputTokens`, `cachedInputTokens`, `cacheWriteInputTokens`, `outputTokens`, `reasoningOutputTokens`, `totalTokens` | No summing repeated totals. Cache/reasoning inclusion and resumed/child scope unverified. `cacheWriteInputTokens` has schema default 0 but does not justify inventing observed zero. |
| Context capacity | Same admitted thread | As above | `modelContextWindow` nullable | Unknown when unavailable; `last` as occupancy needs comparison, not assumption. |
| Rollout records | Header identity plus selected current lineage required | Versioned record identity unverified | Exact trusted path only; history may be paginated | No record parser certified; no imported history or usage asserted. |
| Cost / final accounting | No certified source | No certified source | No cost field in retained token schema | Unknown cost; Partial usage and Observed activity only if later supported. Never infer Complete/Confirmed from exit or EOF. |

## Literal acceptance references

These are requirements, **not passing tests or native traces**:

```text
Selected thread A; foreign/child B startup first => never bind B.
Bind A; child completion B => no root idle/revision/freshness mutation.
Cumulative tokens 100, 100, 140 => 140; replay stays 140.
Two distinct request identities each using20 =>40.
Input100 including cached40; output20 including reasoning5 => total120.
A current; delayed prior-turn completion => no new-turn idle.
```

All six are unrun: no admitted source or adapter exists. In particular the two-request sum is an identified-record alternative, not permission to sum cumulative notifications. Missing metric components remain unknown, never zero.

## Launch grammar and initial native gate checkpoint

| Form | Native grammar evidence | Managed reporting |
| --- | --- | --- |
| Fresh `codex [OPTIONS] [PROMPT]` | Installed help | Unsupported until root/source certification. |
| Explicit `codex resume <id>` | Top-level help names resume; exact subcommand grammar not certified in this pass | Disabled. |
| Picker/last, fork, in-process switch | Mentioned native commands, not investigated for admission | Disabled/deferred. |
| `--remote` | Help lists websocket/Unix endpoints | Does not certify safe passive connection or permit substituting native launch topology. |

At initial checkpoint `d47e99f`, no native attempt had run. Independent review required using the authorized attempt; the [correction](native-attempt/results.md) now records an actual isolated native sign-in selector. The first harness attempt failed at sandboxed process inspection; the single repeat captured onboarding and verified its owned group cleanup. Authentication was not selected. Root hooks, two turns, approval wait/allow, cancellation, root failure, child activity, selected resume, observer disconnect and numeric comparison remain blocked/not run. There is no assistant output marker, screenshot, AX capture, provider request or hook-event fixture. Attempt 01 PID/group cleanup remains unverified; its exact scratch directory is retained.

## Command outcomes and remaining gate

All shell invocations used RTK after the initial instruction read (that read was a plain `cat`, exit 0). Version/help/app-server help/schema generation each exited 0 with the existing PATH-alias `Operation not permitted` warning. Daemon help and migration help exited 0; neither daemon management nor migration was executed. Installed package/hash inspection exited 0. Public release lookup initially failed network access; explicit read-only escalation succeeded (tag/tree/source reads). One guessed source path `core/src/session.rs` returned HTTP 404 and base64 decoding failed; the correct tree-listed `core/src/session/mod.rs` subsequently succeeded. This is retained as a failed source lookup, not a native observation failure or a second comparison pass.

No Rust tests, parser tests, completed native acceptance matrix, Linux gate, hosted capacity job, or 50-session test was run for these documentation/source fixtures. Native startup boundary evidence is recorded separately above. Existing Claude results are not Codex acceptance. Independent review of this source-negative decision remains required.

To unblock, establish an exact passive observer primitive preserving native approvals, **or** certify the existing hook-to-file candidate through a trusted task-owned invocation/root binding, matching current header/lineage and paginated record semantics. The authorized no-credential startup now demonstrably stops at native authentication onboarding. Continuing this matrix requires an authentication route; no credentials or login choice were supplied. This is a concrete prerequisite, not proof that the file route is impossible. Trust was not encountered. The one source pass and native attempt plus permitted repeat are exhausted. Do not start Tasks 2–4 from schema availability alone. Initial explicit resume remains contingent on identity proof; broad transitions, Confirmed completion, Complete accounting, notifications and pricing remain deferred.

Help normalization follow-up: branch-range whitespace validation found 11 trailing-whitespace lines in the original help captures. They were removed from retained help; provenance preserves original raw hashes/scratch paths and updated normalized hashes. No source semantics changed.

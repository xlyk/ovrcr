# Operator runtime and authentication reference

User setup and controls are in [Use the built-in operator](../operator.md).

## Worker and Bash behavior

Each submitted turn runs a separate managed worker, with at most twenty model
calls and one Bash tool at a time. The Server saves streaming text and tool
boundaries before acknowledging them. Completed model context is retained for
the next turn; interrupted commands are never replayed. Stop affects the selected
turn only. OVRCR operations that were already accepted may finish afterward.

The Bash tool accepts `command`, optional absolute `cwd`, and optional
`timeout_seconds` (120 by default, at most 600). It runs
`bash --noprofile --norc -c`, retains at most 64 KiB of combined stdout/stderr,
and records exit status and truncation. It defaults to the captured workspace
or the instance's `operator-workdir`. `$OVRCR_EXECUTABLE` names the matching
absolute CLI; public instance socket/configuration variables are preserved.
Provider and private reporting credentials are removed from the Bash environment;
unrelated user tool credentials remain available. Startup-file environment hooks
are disabled.

Timeout first records the observed partial output and timeout status, then asks
the Server to stop that exact turn and its owned worker/Bash group. The worker
cannot continue or retry an uncertain command. A completed response or worker
socket closure alone is not process-stop proof; the Server's confirmed controlled
stop frees the conversation for another turn.

## Subscription authentication

Operator credentials belong to one OVRCR instance. The operator never imports
personal Codex, Pi or Grok profiles, reads API-key environment variables, or falls
back to API-key billing. Login is explicit; tests use local HTTP fixtures and
synthetic credentials. Live account acceptance is recorded separately from
those fixtures.

ChatGPT uses [Sign in with ChatGPT for open-source clients](https://developers.openai.com/siwc/token-sharing-open-source).
The browser flow registers OVRCR with `dynamic_agent_client`, a persistent
instance host ID, fresh state/nonce and PKCE S256. The loopback listener binds
`127.0.0.1` before the browser opens. The code exchange uses the issued client ID,
the exact callback URI and `resource=https://api.openai.com/v1`. ID-token signature,
issuer, audience, expiry and nonce are validated before credentials become active.
Returning login keeps the selected registration and verified subject separate from
other named profiles. Identity-only consent is saved but cannot authorize inference.
The token grant must include `chatgpt.tokens.use.direct`.

[Account-specific model discovery](https://developers.openai.com/siwc/token-sharing-open-source/models-and-inference)
uses `GET https://api.openai.com/v1/models` with the selected OAuth bearer token.
Only `visibility:list` choices are displayed, preserving server order and slugs.
ChatGPT inference must use `POST https://api.openai.com/v1/responses`, `store:false`
and `stream:true`; successful completion requires `response.completed`. The public
subscription route can omit Content-Type on successful SSE. Only ChatGPT's exact
configured Responses POST with `stream:true` supplies the missing SSE header for
HTTP200; explicit content types, non-200 replies, other routes and Grok are untouched.
The original stream and completion validation remain intact. The worker
uses Rig's public OpenAI Responses provider rather than its legacy ChatGPT preset.
It sends the complete saved SDK history on each HTTP request, including encrypted
reasoning continuation metadata. A narrow outbound transformation groups Bash
under the `ovrcr` tool namespace required by the
[SIWC preview contract](https://developers.openai.com/siwc/token-sharing-open-source/preview-limitations).
It omits unsupported sampling, output-limit and persistent-conversation fields.
Grok retains the normal xAI function-tool encoding. Each model turn must have a
normalized Stop or ToolCalls outcome before tool dispatch or successful
finalization. Incomplete, filtered and unknown outcomes preserve visible partial
text as a failed turn and never save completed model history.

Grok uses the device authorization and refresh flow from
[Pi at f09d01b85e0384647fcab6779d35ab9dabe04be3](https://github.com/earendil-works/pi/tree/f09d01b85e0384647fcab6779d35ab9dabe04be3/packages/ai/src/auth/oauth).
It posts to `https://auth.x.ai/oauth2/device/code`, polls
`https://auth.x.ai/oauth2/token`, handles pending/slow-down/denied/expired/cancelled
states, and refreshes through the same token endpoint. The OAuth access token is
passed directly as the bearer credential to Rig's normal xAI Responses provider at
`https://api.x.ai/v1/responses`. Grok's curated tool-capable choices are
[Grok 4.7](https://docs.x.ai/developers/models/grok-4.7),
[Grok 4.3](https://docs.x.ai/developers/models/grok-4.3-latest) and
[Grok 4.20 Reasoning](https://docs.x.ai/developers/models/grok-4.20).
This catalog is a supported selection list, not evidence of account entitlement.
Only an authorized live request can prove access. The Pi MIT notice is retained in
[third-party notices](../../THIRD_PARTY_NOTICES.md).

Credentials are private `0600` files under the instance's `operator-auth/` directory
(`0700`). The public CLI and workers use the default registration. Internal named profile
identifiers are hashed into filenames. The stable host ID
is shared by these registrations. Symlinked, multiply linked, foreign-owned or
nonprivate files are refused. Updates write and sync a private temporary file,
atomically replace the record, and sync the directory. Per-provider/profile file
locks serialize refresh across processes; the lock is released before returning
a bearer snapshot for inference. Replacement refresh tokens, scopes and expiry
are saved together. Omitted replacement tokens retain the previous refresh token.
Persistence failure returns an error and supplies no new bearer snapshot.

Tokens, login URLs with ID-token hints, auth responses and remote error descriptions
must never enter settings, the conversation journal, diagnostics, terminal commands
or shell environments. Credential-bearing types have redacted `Debug`; errors
contain fixed local messages. Provider failures retain only Rig's fixed classification,
known numeric HTTP status, a fixed response-shape label, exact allowlisted public provider codes
and known request parameter names. Other remote strings and request IDs remain private.
Recovery guidance follows [SIWC errors and recovery](https://developers.openai.com/siwc/token-sharing-open-source/errors-and-recovery);
model discovery alone does not prove inference permission. Failures never automatically
retry, change billing, clear credentials or create a conversation. Worker code must pass bearer snapshots directly to
the SDK without serializing or logging them.

ChatGPT logout attempts renewable-session revocation and clears local access,
refresh and ID tokens while retaining the registration and host ID. The return
value distinguishes confirmed remote revocation from local-only logout; unconfirmed
revocation should direct the user to ChatGPT Settings. Grok logout clears local
credentials and does not claim remote revocation.

## Dependency and runtime boundary

The root application pins `rig =0.45.0` with only `agent`, `reqwest` and `rustls`.
Its released MSRV is Rust 1.97.1. Rig types remain in the root application; shared
protocol/runtime/TUI crates do not depend on Rig. Workers create a Tokio
current-thread runtime with I/O/time enabled. Auth uses that runtime and does not
create its own runtime or background owner.

## Offline acceptance endpoint

Only builds with `acceptance-diagnostics` accept `OVRCR_OPERATOR_TEST_API_BASE`.
The value must be an HTTP URL with a literal loopback IP and no credentials,
query or fragment. Supplying it to a normal build, or supplying an invalid value,
fails the turn before inference. There is no credential bypass: fixtures sign in
through the real AuthManager and use protected synthetic OAuth records. Normal
builds use the fixed production endpoints above.

## Ownership and wire identity

The existing SessionStore owns conversation rows, turns, visible messages and
versioned completed model context. Protocol 68 carries conversation/turn/session/run
identities and bounded sequenced worker batches. Schema 26 retains global
OperatorConversation sessions without registering a project or workspace.

CLI terminal JSON includes the canonical serialized session scope: `"Workspace"`
or `{"OperatorConversation":{"conversation":1}}`. Global operator rows have
empty project/workspace fields. Their original working directory is still
`cwd`. Generic `terminal kill` and
`terminal close` refuse Operator rows; open the Operator conversation and use native
Stop (`Ctrl-x`) to interrupt its exact turn without archiving conversation history.
After independent process-stop confirmation, exact-run `acknowledge-stopped`
interrupts any unfinished turn before recording ownership proof. Live workers and
stale runs remain refused.

The Dashboard receives coalesced dirty notifications and reads durable history.
A summary's `run` is the current retained authority, which can advance beyond
its historical latest turn on archive and restore; `archived` reuses retained
disposition. Native `Close { conversation, expected_run }` uses per-conversation
control admission, validates the current Dashboard owner and exact retained run,
then confirms owned worker termination before archiving. Never-run and already
stopped conversations need no invented turn. Close preserves messages and completed
model history; active work first receives the ordinary durable interruption.
Stop/journal failures leave the conversation unarchived and readable. Termination
runs outside mutation/retained/Session locks, and the final archive revalidates
owner, retained run and stopped proof under retained → Dashboard lock order.

Active listing filters archived rows before pagination; Read remains available.
The Archived sessions palette offers Operator Unarchive, which advances the
retained run without launching a worker. Only a fresh explicit Submit using that
restored run atomically activates the returned record and adds a new turn/message.
Permanent Operator deletion is refused by both the palette and Server.
Archive and Unarchive publish `OperatorChanged` for durable catch-up.

A summary's `worker_stopped` comes from retained controlled-stop proof. Natural
exit, terminal turn state and socket loss do not establish that proof. Model and
Bash waits run in per-turn workers; process termination stays outside dispatcher
and retained/Session locks.

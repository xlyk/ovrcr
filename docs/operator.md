# Built-in operator

## Subscription authentication

Operator credentials belong to one OVRCR instance. The operator never imports
personal Codex, Pi or Grok profiles, reads API-key environment variables, or falls
back to API-key billing. Login is explicit; tests use local HTTP fixtures and
synthetic credentials. Live provider login and inference remain acceptance work
requiring separate authorization.

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
and `stream:true`; successful completion requires `response.completed`. The worker
uses Rig's public OpenAI Responses provider rather than its legacy ChatGPT preset.

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
[third-party notices](../THIRD_PARTY_NOTICES.md).

Credentials are private `0600` files under the instance's `operator-auth/` directory
(`0700`). Named profile identifiers are hashed into filenames. The stable host ID
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
contain fixed local messages. Worker code must pass bearer snapshots directly to
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

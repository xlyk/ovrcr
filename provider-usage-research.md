# Provider usage in OVRCR

Research and design discussion, 2026-09-29. **Not an approved implementation specification.**

**Follow-up boundary:** the source-only/no-private-probe statements below describe the original research pass. The subsequent specification is [#227](https://github.com/xlyk/ovrcr/issues/227), with execution slices [#228](https://github.com/xlyk/ovrcr/issues/228)–[#231](https://github.com/xlyk/ovrcr/issues/231). After the user requested implementation and explicitly authorized existing native logins, the implementation-gate follow-up in the Grok section records bounded authenticated native Codex/Grok compatibility probes. It does not authorize direct credential extraction or internal HTTP readers.

## Scope and evidence boundary

The requested surface is a compact, fixed block at the bottom left of the Dashboard sidebar. The user clarified during this research: **Claude, Codex, and Grok are all required**. A Claude-only release is not the agreed first slice.

OVRCR inspected revision: `14c7fe5ffff7e0dd90a920f2006f35c4b3a97c25`, branch `review/provider-usage-20260929-131144`. The workspace was initially clean.

Superset inspected revision: **`149e20ef52ffda02859cc0eeba60cb3f8fb01889`**, upstream <https://github.com/superset-sh/superset>, committed 2026-09-29T19:47:06Z. Public `git ls-remote` and GitHub commit metadata agreed. Local checkout searches were performed first: only older research notes and unverified Superset worktree/session directories were found. No existing Superset checkout was verified or modified; uncertain/live resources were not used. The investigation instead inspected a task-owned public source download at the pinned upstream revision. The main session is `openai-codex/gpt-6.1-sol`, thinking high; the source-research child uses the same model and thinking level.

Evidence below distinguishes **source facts**, **recommendations**, and **unverified integration questions**. No private usage, credentials, provider account files, or live terminal output were inspected. No Superset installation or execution, provider conversation, configuration change, commit, push, issue, or PR is part of this research.

## Recommendation in brief

Keep the selected **Quota left** bars, separate **5h/7d** rows for Claude and Codex when those windows are reported, and Grok's actual weekly/monthly period. Track account allowance, not session tokens, estimated spend or context occupancy. Use one Server-owned snapshot and the existing Dashboard delivery/layout mechanisms.

Superset is useful as UI/cache/schema evidence, not as a ready-made provider contract. Reuse Claude's native status-line quota fields; prefer Codex's documented app-server rate-limit RPC; validate Grok's typed native billing route before accepting a three-provider implementation. Do not copy Claude OAuth credential forwarding or Superset's heuristic Grok protobuf scanner. Supporting all three remains the requested first slice, with Grok access and Claude profile attribution the main unresolved integration questions.

## What “usage” must mean

Four different quantities must not be combined:

| Quantity | Meaning | Suitable for the requested provider block? |
| --- | --- | --- |
| Subscription/account allowance or rate-limit quota | Provider-reported percentage consumed in a named window, with a provider reset time | Yes; the proposed scope |
| Conversation/invocation tokens | Input/output totals for an individual agent conversation or process | No; keep existing session inspection/header reporting |
| Estimated spend | Token-derived or source-reported estimated dollars; not necessarily the subscription bill | No; a separate metric with its existing estimate label |
| Context occupancy | Tokens currently occupying one model context relative to capacity | No; not remaining account allowance |

Do not sum quotas across sessions. Two sessions can consume the same account allowance. Two different logins can have different allowances even when both run the same provider. A percent with no window, source, or account/profile provenance is ambiguous.

The user selected **Quota left**: bars and percentages show remaining allowance, with separate **5h** and **7d** windows wherever the provider reports those periods. For an ordinary 0–100% consumed allowance, remaining is `100 − used`. Missing data is `—`, never a fabricated empty/full bar. Preserve source zero correctly: 0% used means 100% left; 100% used means 0% left. Do not relabel a Grok weekly/monthly period as a five-hour quota.

## What Superset actually implements

### UI and complete quota data path

**Source facts:** Superset's sidebar Usage control navigates to settings; it is not an inline bottom-left quota panel. Expanded/collapsed sidebar controls route to the last used section, `/settings/usage` or `/settings/usage/resources`, recorded in `usage-last-section-v1` localStorage. A stored `showUsageInSidebar` setting controls that navigation entry. [SS1], [SS2], [SS31], [SS32]

The authenticated Token usage page selects the active host, or a workspace's host when visited with `workspaceId`, `accountKey`, and `agent` search fields. It renders `UsageView`, which calls `useHostUsageQuota(hostUrl)`. The hook invokes the host tRPC `usage.quota` procedure at `${hostUrl}/trpc`; the host calls `getQuota()`, which runs five adapter groups in parallel and returns normalized `UsageAccount[]`. Host RPC authentication uses a per-host secret/sandbox token or Superset JWT for relay access. Those credentials are separate from the provider OAuth credentials read inside the host. Data is per host, not an aggregate across hosts. [SS3], [SS4], [SS5], [SS6], [SS30]

Account cards show provider/harness, identity, plan, credential source, status, quota windows, and optional credits/extra spend. Claude/Codex sections remain visible with no login so Add account is available; Grok, Antigravity and OpenCode sections appear only after account discovery. Bars show **used** percentage, not remaining allowance. Fill caps at 100%, while numeric over-limit percentage can remain visible; amber starts at 70%, red at 90%. [SS7], [SS8]

Superset also has a Claude/Codex terminal-header account ring. It matches validated launch-account identity to the shared quota snapshot, says the allowance is shared across sessions using that login, and links to settings. It is not a session token or context meter. Account identity polls every 30 seconds; quota reuses the five-minute hook. Its separate 30-second presentation clock marks data stale after query error, ten minutes, or a passed reset. The main Usage page has different stale/error behavior, described below. [SS9], [SS33]

### Provider adapters, data sources and authentication

All quota HTTP fetches have a ten-second timeout. No authenticated request was made during this research.

| Superset quota agent | Source-only credential requirements | Request and actual meaning |
| --- | --- | --- |
| Claude Code | Existing Claude Code OAuth access token from macOS Keychain, default credential files, `CLAUDE_CONFIG_DIR` overrides, or discovered profiles; local expiry/identity metadata | GET `https://api.anthropic.com/api/oauth/usage`, Bearer token plus `anthropic-beta: oauth-2025-04-20`; separate GET `/api/oauth/profile` for email. Maps `five_hour`, `seven_day`, `seven_day_sonnet`, and model-scoped weekly `limits[]`; ISO reset timestamps. This is subscription quota. Optional extra usage is separately mapped from credits/monthly allowance. Source explicitly calls the endpoint undocumented. [SS10] |
| Codex | File-based OAuth in `$CODEX_HOME/auth.json` or discovered Codex homes, `tokens.access_token`, optional `tokens.account_id` | GET `https://chatgpt.com/backend-api/wham/usage`, Bearer plus optional `chatgpt-account-id`. Maps primary and secondary windows using actual `limit_window_seconds`, `used_percent`, epoch `reset_at` or relative `reset_after_seconds`; additional buckets' primary windows only. Plan/email/credits come from the response. No adapter support for OS-keyring/ephemeral-only auth. [SS11] |
| Grok | First entry with `key` in `~/.grok/auth.json`, with local email/tier/expiry; one login, no account override enumeration or API-key fallback | POST `https://grok.com/grok_api_v2.GrokBuildBilling/GetGrokCreditsConfig` with Bearer, Origin/Referer and gRPC-web headers, empty five-byte frame. Heuristically extracts one percentage/reset from protobuf and labels it Weekly. This is source-attested Grok Build/SuperGrok account allowance, **not a verified public API or 5h/7d contract**. [SS12] |
| Antigravity | macOS Keychain service `gemini`, account `antigravity`; OAuth access token/expiry/email | POST Google's `v1internal:retrieveUserQuotaSummary`, trying daily-cloudcode then cloudcode hosts. Maps five-hour/weekly Gemini and third-party buckets from `remainingFraction`. No Linux credential implementation in this adapter. [SS13] |
| OpenCode | OAuth/API-type entries for Anthropic/OpenAI in its XDG data `auth.json`; not arbitrary model providers | Reuses the same Claude/Codex quota fetchers. API-type entries show API-billed/no-quota cards, not subscription meters. No account-management actions. [SS14] |

Claude/Codex discovery includes bounded profile enumeration and deduplication, not every possible config or auth mode. Claude combines default Keychain/file candidates into one login slot, favors live/fresher credentials, and dedupes additional tokens. Codex checks ambient/default home first and dedupes same-email accounts to the first home. Superset's `.superset-api-billing` marker enables API-billed cards without a quota fetch; it does not establish universal API-key discovery. API cards say “Billed per token” and link out to provider consoles rather than calling organization usage APIs. [SS10], [SS11], [SS15], [SS7]

**Token policy:** these passive quota adapters do not refresh provider tokens. Claude distinguishes expired access with a known live refresh token (`token_stale`, refreshed when the native CLI next runs) from expired/unusable login (`token_expired`). Other providers have narrower classifications. Superset's comments cite refresh-token reuse/sign-out risks as its reason not to compete with native CLI refresh. [SS10], [SS11]

**Separate mutation functionality:** Claude/Codex account dialogs can run the native login command, add profile roots, change defaults, prepare configuration/history, remove secondary profiles, and offer session restart. Login discovery polls every three seconds while the dialog waits. These capabilities are not part of the passive adapter and are not appropriate dependencies for OVRCR's first quota slice. Grok has no Superset add/default/remove flow. [SS6], [SS7], [SS16]

### Polling, caches, resets and errors

* **Host cache:** a module-memory promise for the entire account list, five-minute TTL from request start; ordinary concurrent reads share it. Adapters normally convert failures to account error statuses, so 401/429/unavailable results are cached for five minutes too. A rejected overall promise is evicted. No persisted host quota history. [SS6]
* **Force refresh:** bypasses even a fresh/in-flight cache entry and replaces it. No independent forced-request cooldown or rate limiter. Five-minute polling is therefore not a hard maximum request frequency. [SS6]
* **Renderer:** query key includes host URL; five-minute polling and staleTime, 24-hour in-memory gcTime. Quota does not enable background interval refetch. Electron focus can trigger stale refetch; the persistence whitelist excludes quota, so 24 hours is not cross-restart storage. [SS4], [SS17]
* **Timeouts:** quota RPC budget 15 seconds; provider fetch budget ten seconds. Host timeout middleware races work but does not cancel it. A timed-out query can leave its cache promise resolving in the background. Classified host connection failures get bounded client retries; provider adapters have no dedicated Retry-After/backoff policy. [SS5], [SS6], [SS18]
* **Reset windows:** adapters use provider reset timestamps. Main-page countdowns round to minutes and become “now” after reset, with an absolute-time tooltip. `UsageView` has no dedicated ticking clock, no special reset-triggered fetch, and never resets percentage to zero locally. Terminal-header presentation has the separate clock/stale behavior noted above. [SS7], [SS9], [SS19]
* **Initial state:** visible sections say “Reading usage…”; no account says “No … logins on this host.” Main `UsageView` does not distinguish a first whole-query transport error from an empty result, so an error can misleadingly look like no logins. [SS7]
* **Refresh failure:** failed whole-query refresh retains prior cards without a main-page freshness badge or explicit refresh-error toast. A successful query containing provider-error cards replaces prior good windows with those errors. HTTP 401 **or 403** is labelled token-expired; other HTTP/network/parse failures become unavailable. A 429 is generic unavailable, not a distinct rate-limit/backoff state. Missing/unreadable Grok auth returns no account and hides that section. [SS4], [SS7], [SS10], [SS11], [SS12]

Superset's host comment says faster polling can cause Anthropic 429 blacklisting and cites community incidents. That is its design rationale, **not** an official five-minute safe-polling guarantee. OVRCR should preserve last-good values with explicit age/error state and apply bounded refresh/backoff rather than copy these failure ambiguities.

### Grok's parser is a material reliability gap

Superset scans gRPC-web data frames, ignores trailer frames, and recursively searches protobuf fields to depth four. It selects a finite [0,100] fixed32 float ending in field 1, preferring a shorter path; it picks a future epoch varint, preferring path `1.5.1`, as reset. The adapter emits exactly one Weekly window, with no duration field, no monthly period mapping and no five-hour window. Its single synthetic parser test uses 42.5% and a nested future reset; it is not live entitlement/schema verification. Wrong-field matches, compressed frames and trailer errors are unresolved risks. [SS12], [SS20]

The parent inspected pinned native Grok source independently: its typed billing path has `creditUsagePercent` and explicit weekly/monthly `currentPeriod`, as documented below. That native evidence must not be attributed to Superset's heuristic parser. Prefer the typed native route if safely accessible; do not transplant the heuristic into OVRCR merely because Superset contains it.

### The rest of the Usage page is a different product

The Token usage section also includes historical tokens/cost charts, a leaderboard, and drilldowns. History defaults to 30 days with 7/30/90-day choices; `usage.history` coalesces worker scans, five-minute staleTime and 24-hour memory caching, but no interval poll. Claude/Codex primarily scan local session logs with realpath deduplication. Cursor is an exception: it fetches authenticated usage-event history from a Cursor backend and uses local metadata for attribution. Twelve history harnesses are supported, distinct from the five quota agents. [SS6], [SS21], [SS22], [SS24], [SS29]

List-rate pricing normally estimates cost, with approximate fallback for unknown models; source-reported `costUsd` can override estimation. The Cost-mode “Cost to you: $0” caption is unconditional, not verified subscription/API billing. The page does not derive account quota from token totals and has no context-window-remaining gauge. Its leaderboard uses separate Superset API calls and opt-in publication; Machine resources is separate CPU/RAM/process telemetry. These features are out of scope for OVRCR's sidebar. [SS21], [SS23], [SS25], [SS26], [SS27], [SS28]

### Provider support and policy are not established by Superset code

[Anthropic's official credential-use policy](https://code.claude.com/docs/en/legal-and-compliance#authentication-and-credential-use), independently fetched by parent and child, says third-party developers may not collect/store/intermediate Claude.ai credentials/session tokens or offer Claude.ai login in their own apps. It distinguishes end-user sign-in to unmodified Claude Code. **Do not adopt Superset's Claude credential read-and-forward design without resolving this constraint.** This is a design constraint, not a legal judgment about Superset. Native status-line quota observations avoid making OVRCR another Claude OAuth client.

Official [Anthropic usage/cost](https://platform.claude.com/docs/en/manage-claude/usage-cost-api) and [OpenAI organization usage](https://developers.openai.com/api/reference/resources/admin/subresources/organization/subresources/usage/methods/completions) APIs concern organization API usage/billing, not Pro/Max or ChatGPT subscription quota. Their API throughput limits are also distinct from subscription allowance. Codex's documented app-server account RPC is a better candidate than directly reproducing `wham/usage`.

Official [xAI rate limits](https://docs.x.ai/developers/rate-limits), [Management API](https://docs.x.ai/developers/management-api-guide), and [billing reference](https://docs.x.ai/developers/rest-api-reference/management/billing) concern API throughput/team invoice/spending/prepaid balance, not the private grok.com Build RPC. Official [Grok CLI commands](https://docs.x.ai/build/modes-and-commands) distinguish account credit/billing `/usage` from `/context`; this does not validate Superset's parser. Consumer pages returned 403 or a content shell during unauthenticated research, so they did not establish entitlement, duration, polling permission or response-schema guarantees.

## OVRCR today: reusable mechanisms and missing pieces

### Reporting path

**Source facts:** [protocol agent types](crates/ovrcr-protocol/src/agent.rs) define `UsageTotals` with Conversation/Invocation scope, Complete/Partial coverage, and separate `UsageCost` and `ContextSample`. There is no account-quota scope or reset-window type. `AgentProvider` identifies the agent harness: Pi/OMP are harnesses, not billing accounts.

Managed Claude already forwards the original bounded status-line JSON through [`run_claude_statusline`](src/cli/report.rs#L152), [`send_claude_statusline`](src/report.rs#L541), and the authenticated invocation channel. [`Hooks::statusline`](src/report/admission.rs#L697) verifies the bound conversation, normalizes model/context/cost, and publishes through the shared Reporter. [`parse_claude_metrics`](src/report/claude_metrics.rs#L46) currently does **not** parse account quota. Recognized transcript records separately supply Partial conversation tokens. Cost is Estimated conversation cost, not quota or a verified invoice.

Codex's [`publish_model`](src/report/codex.rs#L353) and the shared Pi/OMP [`publish_model`](src/report/extension.rs#L453) emit model-only Metrics samples with unknown token, cost, and context values. See [Codex setup](docs/codex-reporting-setup.md#what-the-indicator-means) and [Pi setup](docs/pi-reporting-setup.md#what-is-reported). Managed [Grok reporting](src/report/grok.rs) currently retains a conversation/history reference for titles and uses a silent receiver; it has no quota feed.

Accepted observations update the Session and reach the Dashboard through the existing [`SessionChanged` publication](crates/ovrcr-runtime/src/server/dispatch.rs#L365). [`ovrcr session usage`](src/cli/resources.rs#L524) inspects those session observations; it does not query provider account limits.

**Recommendation:** preserve that session path. Add a distinct quota observation/snapshot only after design approval; do not relabel `UsageTotals`, poll transcripts for account quota, or add quota lifecycle logic separately to every receiver. The Reporter remains the owner of managed invocation reporting. An account collector, if selected, belongs to Server-owned background work, not to a new multiplexer server or the Bridge.

### Freshness is not interchangeable

[`freshness.rs`](crates/ovrcr-protocol/src/freshness.rs) labels existing session metrics stale after five minutes, after session exit, or with an unverifiable future receipt time. An identical sample keeps its original receipt timestamp; a changed value advances it. This is deliberate session-reporting behavior.

**Recommendation:** reuse the age calculation and five-minute display vocabulary, but not the whole session rule blindly. A successful account API fetch can verify an unchanged quota and should advance its `last_checked` timestamp. Native event-driven reports may merely repeat a cached sample; they do not prove a new backend check. Quota snapshot provenance should distinguish report receipt from verified account fetch. Quota validity is independent of one session exiting, although loss of its only native observation source must be shown.

### Sidebar layout

[`dashboard_layout`](crates/ovrcr-tui/src/dashboard/render.rs#L919) currently reserves a one-line title and global footer. The sidebar tree owns the full intervening sidebar height. Default sidebar width is 40 cells, excluding its one-cell divider; drag minimum is 20, but very small windows can force less than that through the half-window cap ([width rules](crates/ovrcr-tui/src/dashboard/mod.rs#L118)). `b` hides the sidebar for the current attachment and returns its width to panes ([state](crates/ovrcr-tui/src/dashboard/state.rs#L2068)).

The tree may stack agent/model labels into two lines. Drawing, scrolling, selection visibility, hover, close marks, and mouse selection share row-height helpers. [`tree_viewport_height`](crates/ovrcr-tui/src/dashboard/state.rs#L1155) currently derives tree height from pane sizing; [`sidebar_mouse_action`](crates/ovrcr-tui/src/dashboard/state.rs#L2128) treats the sidebar content rectangle as tree space.

**Recommendation:** split one shared sidebar geometry into tree and quota rectangles. Every tree viewport and hit-test consumer must use the reduced tree rectangle. Never paint a floating quota block over still-selectable session rows. This need does not justify changing pane geometry, session status, or the global key-hint footer.

## Provider-owned alternatives to credential scraping

These are alternatives for OVRCR, not claims about Superset's implementation.

### Claude: documented native status-line quotas

**Official documentation:** [available data](https://code.claude.com/docs/en/statusline#available-data), [rate-limit usage](https://code.claude.com/docs/en/statusline#rate-limit-usage), and [update behavior](https://code.claude.com/docs/en/statusline#how-status-lines-work), fetched 2026-09-29. The Markdown form is also available at <https://code.claude.com/docs/en/statusline.md>.

The native JSON supplies `rate_limits.five_hour.used_percentage`, `rate_limits.seven_day.used_percentage`, and each window's `resets_at` in Unix seconds. Documentation calls these a rolling five-hour window and a weekly seven-day window. This is **subscription quota**, not `context_window.used_percentage`.

The docs say `rate_limits` appears only for claude.ai Pro/Max subscribers, or a Claude apps gateway with spend limits, and only after the first API response. Windows may independently be absent. Claude drops a window after its reset time passes. Gateway `spend_limit` is a different allowance and can exceed 100%; do not mislabel it as a five-hour subscription window.

Native status-line executions are event-driven, debounced at 300 ms, include reset-time triggers, and can have an optional refresh interval. An idle native session is not continuous account polling. OVRCR must not force paid work to obtain an observation.

**Feasibility:** smallest existing seam for Claude, with no new credential access. This checkout's Claude compatibility minimum is 2.1.267 ([policy](src/report/versions.rs#L8)); the current docs are not proof that quota fields were captured or accepted by the repository's earlier native tests. The JSON has session identity but does not establish a shared account identity across sessions. A global row needs explicit profile attribution or must label its session provenance instead of merging all Claude reports.

### Codex: documented account-rate-limit RPC

**Official documentation:** <https://developers.openai.com/codex/app-server/>, fetched 2026-09-29. Public upstream `openai/codex` main resolved to `804d6306e84f570393cdd1eec94c41464f503b1a` during this pass.

The app-server documents `account/rateLimits/read` and `account/rateLimits/updated`. Window fields include `usedPercent`, `windowDurationMins`, and `resetsAt` in Unix seconds. The backwards-compatible single view is `rateLimits`; `rateLimitsByLimitId` can contain multiple metered buckets. Do not assume primary always means five hours or secondary always means a week; use actual durations and bucket identifiers. Optional plan and credits fields are separate from percent usage.

The same docs separately describe `account/usage/read` for lifetime/daily token activity. That is **not** the quota RPC and is outside this sidebar's first slice. Reset-credit consumption and billing actions are also outside scope.

**Feasibility:** prefer the documented RPC over reading OAuth tokens and reproducing ChatGPT HTTP endpoints. OVRCR's current hook receiver is not an app-server client and cannot obtain this through its model-only Metrics feed. A bounded, read-only app-server integration would need validation with the selected native profile/auth mode and installed version. It must not create a conversation, take over a live Codex client, restart an existing daemon merely for quota, or claim API-key-only billing quota is ChatGPT subscription quota. Token refresh behavior and file writes by the native client require explicit review before calling the integration non-mutating.

### Grok: account billing is real, but not in the existing reporting path

Verified public upstream: `xai-org/grok-build`, commit `2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8`. This public snapshot is not a proven match to the user's installed Grok binary.

* [Native billing extension](https://github.com/xai-org/grok-build/blob/2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8/crates/codegen/xai-grok-shell/src/extensions/billing.rs#L27-L109) defines `creditUsagePercent`, weekly/monthly `currentPeriod` with RFC3339 start/end, on-demand amounts, and prepaid balance. [The handler](https://github.com/xai-org/grok-build/blob/2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8/crates/codegen/xai-grok-shell/src/extensions/billing.rs#L182-L259) requires grok.com auth and fetches the CLI proxy's `/billing?format=credits` with a Bearer token and provider identity/client headers; it has a 15-second timeout. This is first-party implementation evidence, not a general public third-party billing API guarantee.
* [Native allowance rendering](https://github.com/xai-org/grok-build/blob/2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8/crates/codegen/xai-grok-pager/src/views/usage_modal.rs#L753-L855) distinguishes consumer allowance, team-managed limits, gateway chat with no Build coding credits, billing redirects, loading, errors, and no data. It renders actual weekly/monthly periods, percent, reset, prepaid credit, and separate pay-as-you-go usage.
* [`grok usage <session-id> [turn]`](https://github.com/xai-org/grok-build/blob/2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8/crates/codegen/xai-grok-pager/src/usage_cmd.rs#L1-L63) reads persisted **session token/cost** data. It is not an account-quota command.
* [Structured status-line fields](https://github.com/xai-org/grok-build/blob/2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8/crates/codegen/xai-grok-status-line/src/context.rs#L18-L46) contain model/context/cost but no account billing/allowance field. Reusing a Grok status-line callback alone cannot provide the requested quota.
* [Authentication guide](https://github.com/xai-org/grok-build/blob/2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8/crates/codegen/xai-grok-pager/docs/user-guide/02-authentication.md#L287-L299) distinguishes per-model keys, active session tokens, `XAI_API_KEY`, enterprise and external auth. A discovered personal login need not be the account a specific running Grok model actually uses.

**Implementation-gate follow-up (2026-09-29):** the pinned [ACP agent dispatcher](https://github.com/xai-org/grok-build/blob/2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8/crates/codegen/xai-grok-shell/src/agent/mvp_agent/acp_agent.rs#L1983-L2013) exposes the billing handler as an extension method, with [direct routing](https://github.com/xai-org/grok-build/blob/2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8/crates/codegen/xai-grok-shell/src/agent/mvp_agent/acp_agent.rs#L2266-L2268). The billing handler itself does not require creating or loading a conversation. This establishes a candidate native ACP path, not installed-version compatibility or side-effect-free startup. [Native agent startup](https://github.com/xai-org/grok-build/blob/2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8/crates/codegen/xai-grok-pager-bin/src/main.rs#L1260-L1455) resolves remote settings/config and may connect to or spawn a shared leader and run auto-updates. Explicit standalone/no-auto-update settings need installed-version validation. The billing handler also emits a structured billing snapshot to the [native unified log](https://github.com/xai-org/grok-build/blob/2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8/crates/codegen/xai-grok-shell/src/extensions/billing.rs#L249-L259); its [writer](https://github.com/xai-org/grok-build/blob/2bdd1d6a6369de0e8c68132ea4539e9abd9e14a8/crates/codegen/xai-grok-telemetry/src/logs/unified_log.rs#L154-L160) targets the native Grok home. Profile isolation and unavoidable native auth/log side effects remain gates. Executable symlink metadata initially identified installations labelled Grok 1.0.40, Codex 0.155.1, and Claude 2.1.285. Following explicit user authorization to use existing native logins, bounded probes in a task-owned workspace confirmed those native versions. A standalone Grok ACP process with auto-update disabled returned a typed `x.ai/billing` response containing a numeric credit percentage and explicit period type/end; an owned Codex app-server returned typed account-rate-limit windows and a bucket map. Neither probe created/loaded a conversation, sent a prompt, extracted credentials, attached to a shared leader, or printed raw provider responses or allowance values. Only sanitized compatibility booleans were recorded, and the task-owned process groups were stopped. Native auth-refresh/billing-log effects were explicitly authorized; native-owned logs were not inspected or copied. This validates the current native routes under that approved scope, not arbitrary versions/profiles or a third-party direct HTTP reader. Claude's quota-frame acceptance remains to be verified through its native reporting integration; no paid Claude call was forced.

**Feasibility:** investigate the native read-only `x.ai/billing` route before inventing a new OAuth store. Safe access without attaching/mutating an agent is unverified. If that is unavailable, investigate a provider-supported quota export or a narrowly approved reader of the typed native proxy JSON response. Do not copy Superset's heuristic protobuf scanner as the default fallback. Any direct internal endpoint/credential use still needs provider-permission, schema, native auth/refresh ownership and user opt-in review; it is not established as support by this source research. Do not substitute xAI API request rate limits, session dollars, consumer message limits, or a guessed weekly cap.

## Sidebar presentations

All numbers below are synthetic. All three providers remain visible; only configured supported accounts produce values. Account/profile identity belongs in details or a short alias when needed.

### 1. Three plain rows — recommended

```text
Quota used
Claude  5h 42% · 7d 18%
Codex   5h 67% · 7d 31%
Grok    week 24%
```

Four lines at the normal width. Percent/window pairs are explicit, and the Grok period changes to `month` when the source says monthly. No charts, auto-rotation, or combined score. Reset times, last-check age, source, and selected profile are available through one keyboard-accessible details action in the existing menu/palette and optional block click. An unavailable row says, for example, `Grok — sign-in needed`; a stale row says `Grok week 24% stale`.

### 2. Reset-aware rows

```text
Quota used
Claude  5h 42% · 7d 18%
        5h resets in 2h 10m
Codex   5h 67% · 7d 31%
        5h resets in 48m
Grok    week 24%
        resets in 3d 6h
```

Seven lines. Better if the user's main question is “when can I work again?”, but takes three additional tree rows and still cannot display every secondary reset comfortably. No local prediction of quota replenishment: countdown reaching zero changes to `reset due`, not invented `0%`.

### 3. Small bars, one primary window per provider

```text
Quota used
Claude  5h  [####......] 42%
Codex   5h  [#######...] 67%
Grok    wk  [##........] 24%
```

Four lines, easy to scan, but loses secondary windows and spends width on decorative cells. Details become necessary. Text percentages must remain available; color/bar fill cannot be the only signal.

### Additional options requested by the user

The user did not select one of the initial three layouts and requested several more. These remain discussion options, not approved UI.

**4. Aligned quota table — revised presentation recommendation**

```text
Quota used    5h    7d
Claude       42%   18%
Codex        67%   31%
Grok         week 24%
```

Four lines, clearer comparison than repeated window labels. The Grok row intentionally names its own period instead of aligning a potentially monthly allowance under `7d`. Actual Codex durations may require different labels or a provider-specific row.

**5. Percent and reset together**

```text
Quota used · ↻ reset in
Claude  5h 42%↻2h  7d 18%↻4d
Codex   5h 67%↻48m 7d 31%↻2d
Grok    week 24%↻3d
```

Four lines with resets always visible, but denser and less suitable at the drag minimum. Use actual terminal width; the reset glyph needs accessible text and must not look like a clickable refresh control unless it is one.

**6. Remaining allowance table**

```text
Quota left    5h    7d
Claude       58%   82%
Codex        33%   69%
Grok         week 76%
```

Answers “how much room do I have?” Derived from provider-reported used percent only when that window's semantics permit it. Never mix `left` and `used` values without labels, or hide over-limit usage by silently clamping it into an ordinary zero.

**7. Window-first comparison**

```text
Used    Claude  Codex  Grok
5h         42%    67%     —
7d         18%    31%   24%
```

Three lines when all reported periods match these windows. A monthly Grok period requires a separate `month` row. Codex's actual durations/buckets can also require extra rows; this layout must not normalize unequal windows as equal.

**8. Closest limit highlighted**

```text
Least headroom: Codex 5h · 33% left
Claude  5h 42% · 7d 18% used
Codex   5h 67% · 7d 31% used
Grok    week 24% used
```

Highlights the fullest valid reported window, not a prediction of exhaustion or a common workload capacity. Exclude stale/unknown windows from the headline and expose how it was selected. More derived behavior than the basic table; defer unless this decision aid matters to the user.

### Further bar options requested by the user

The user also requested several bar-based layouts. These are alternatives, not approval of a bar design. All percentages are synthetic; fill is approximate and the numeric percentage remains authoritative.

**9. Two mini-bars per provider row**

```text
Quota used
Claude 5h ▰▰▱▱▱ 42%  7d ▰▱▱▱▱ 18%
Codex  5h ▰▰▰▱▱ 67%  7d ▰▰▱▱▱ 31%
Grok   wk ▰▱▱▱▱ 24%
```

Four lines with both Claude/Codex windows. Five segments are coarse; percentage text is essential. This fits a normal sidebar better than two full bars but needs a numeric-only fallback at narrow widths.

**10. Stacked windows — recommended if bars are chosen**

```text
Quota used
Claude 5h [████░░░░░░] 42%
       7d [██░░░░░░░░] 18%
Codex  5h [███████░░░] 67%
       7d [███░░░░░░░] 31%
Grok   wk [██░░░░░░░░] 24%
```

Six lines, consistent bar length, explicit windows, and better readability. Costs two more tree rows than the basic numeric table. A separate monthly period must say `mo`, not `wk`.

**11. Remaining-allowance bars**

```text
Quota left
Claude 5h [██████░░░░] 58%
       7d [████████░░] 82%
Codex  5h [███░░░░░░░] 33%
       7d [███████░░░] 69%
Grok   wk [████████░░] 76%
```

Six lines. Full means more room to work, unlike the used-percent layouts. Only compute remaining allowance where the provider's semantics support the subtraction; preserve over-limit states explicitly.

**12. Primary window bar with reset**

```text
Quota used             Reset in
Claude 5h [████░░░░░░] 42%   2h
Codex  5h [███████░░░] 67%  48m
Grok   wk [██░░░░░░░░] 24%   3d
```

Four lines. Secondary windows move to details. Show actual source durations and reset times; never infer a reset from a generic primary-window name. Short countdowns are rounded display values, not predicted account replenishment.

For every bar layout, missing data displays `—` with no fabricated empty bar. Stale samples keep a visible stale marker; shorten or omit the bar before truncating that marker or the window/percentage. Percentages and accessible text must work without color. Reuse Ratatui drawing primitives; no new chart dependency. Native glyph widths and rendering remain a future CUA requirement.

**Selected presentation:** remaining-allowance bars (option 3 in the latest chat list, option 11 in this document), with separate 5h and 7d rows. This supersedes the earlier numeric-table recommendation. Preserve all three providers and the two reported Claude/Codex windows. In a roughly 19-cell interior, keep the `Quota left` heading, shorten provider labels consistently, and shorten or remove the decorative bars before removing window labels, percentages, or stale markers. Do not collapse 5h and 7d into one ambiguous bar. Use `5h`/`7d` only when supported by actual source windows; Grok keeps its actual period. For extremely small heights, collapse to a single `Quota: details` line or omit the block if even that prevents a navigable tree. Do not overwrite the global footer. Hidden sidebar means hidden quota block; details remain available through Browse menu/palette, with no forced reappearance or new terminal-input interception.

## Collection approaches considered

1. **Native-first hybrid — recommended.** Claude supplies quota through OVRCR's existing status-line receiver; Codex supplies account-rate-limit data through its documented RPC; Grok supplies typed native billing data if a safe read-only access path is validated. Reuses existing reporting/worker boundaries and keeps provider credentials managed by native tools where possible. Main limitation: Claude can remain unknown until a managed session reports, and Grok's non-mutating access path remains unverified.
2. **Copy Superset's direct account pollers — not recommended.** Can populate from discovered logins without a running conversation and looks superficially compact. It makes OVRCR a credential-reading OAuth client, conflicts with the reviewed Anthropic policy unless permission is resolved, misses some auth modes, and inherits a fragile Grok heuristic. Superset source is not authorization or a stable API contract.
3. **Require structured provider exports for all three.** Cleanest long-term ownership if each native provider exposes quota without credential forwarding. Avoids internal HTTP and heuristic parsing, but could require Grok/upstream work and cannot be promised from currently verified interfaces. Session token/cost reporting is not an acceptable substitute while waiting, because it does not meet the user's account-allowance requirement.

## Architecture recommendation for the required three-provider slice

**Proposed, not approved:** a small Server-owned quota snapshot keyed by provider and an explicitly selected native account/profile. No multi-account manager, provider login UI, billing dashboard, historic spend database, or generic telemetry platform in the first slice. If multiple profiles are discovered, require selection rather than summing or silently choosing a login. Credentials stay out of wire snapshots, SQLite, captures, diagnostics, and UI.

Use Claude's existing native status-line path where it can be safely attributed to the selected profile. Prefer Codex's documented read-only RPC. Grok needs a separately validated native billing read or an approved reader of the typed native proxy JSON. The Superset private-RPC heuristic is not the recommended implementation. Claude OAuth read-and-forward is excluded unless the official credential-use restriction is resolved. All three must clear their own feasibility/authentication gates before calling this a supported three-provider release; an empty Grok placeholder alone does not satisfy the user's requirement.

For account collectors, follow the existing synchronous worker pattern: [`startup.rs`](crates/ovrcr-runtime/src/server/startup.rs#L209) runs workspace observation outside the dispatcher, and the dispatcher publishes cached results through the sole Dashboard writer. Reuse that ownership/message pattern, not the two-second Git cadence or the same blocking Git worker. Provider network/RPC work must stay off the PTY parser, dispatcher, draw path, and socket writer.

A proposed account refresh policy is one bounded request per selected provider/profile on attach when due, then a conservative five-minute cadence while a Dashboard is active; use native update notifications where available. This is a design default, not a provider-certified safe interval. Native Claude status-line observations remain event-driven, without an OVRCR OAuth poller. Manual refresh must coalesce with in-flight work and honor the same cooldown/backoff, unlike Superset's cache-bypassing force refresh. Keep one in-flight fetch per profile, coalesce refresh requests, use bounded response/time limits, and back off failures/429s. Fifty sessions must not mean fifty account polls. Retain a bounded in-memory cache across Dashboard detach/reattach; no durable quota history. Pause periodic remote polling with no active Dashboard. Hiding the sidebar need not destroy cached state; a details view uses the same collector rather than a second poller. Native Claude reporting continues independently, because it belongs to the managed invocation.

Fence late collector results with the selected profile/source generation. A removed profile or account switch cannot accept an old response. Use the existing Dashboard queue and attachment snapshot path for delivery; no independent socket writer or second active Dashboard. Labels/countdowns can reuse existing redraw timing without issuing provider calls on every draw.

### Missing, stale and error handling

* Initial absent data: `— waiting for report`, `— not signed in`, `— unsupported auth`, or `— unavailable`, according to actual evidence. Absent fields do not prove sign-out.
* Valid ordinary allowance: 0% source usage renders 100% left; 100% source usage renders 0% left. A missing percentage remains unknown, not either boundary. An over-limit source value needs an explicit over-limit state.
* Missing individual window: show only valid named windows; do not manufacture a weekly window or capacity.
* Last good sample after offline/timeout/429: retain it with `stale` and last-check age in details. Backoff and Retry-After must govern further account polls.
* 401/403: show auth unavailable; do not log response bodies or launch an interactive login automatically. Do not silently overwrite native credentials or refresh tokens from a competing collector.
* Reset time reached: mark the previous window expired/reset due until authoritative replacement. Claude explicitly removes expired native windows. A reset is not proof of unused allowance elsewhere.
* Malformed, nonfinite, negative, or mismatched data: reject the affected observation; preserve unrelated provider state. Do not let an optional quota parse error disable existing activity/context/cost reporting.
* Source changed or account attribution uncertain: stop representing the old sample as the selected account's current quota. Show provenance/conflict in details.

## Decisions and verification still needed

**Settled by the user:** all three providers, Claude/Codex/Grok, are required. After considering the additional numeric and bar mockups, the user selected the latest chat's option 3: **remaining-allowance bars**, with **both 5h and 7d shown**. The questionnaire preview explicitly showed Grok with its actual weekly period. The user also selected **native Claude reports only**: its row may wait for a managed Claude session's native quota report, with waiting/stale states. No independent Claude OAuth reader or forced model call is required or authorized. These are design choices, not approval to implement or to access credentials.

**Still proposed for approval:** account/subscription allowance, not session metrics; provider-specific periods when they differ from 5h/7d; one explicitly selected profile per provider; Codex/Grok collection/authentication ownership and the shared quota snapshot; no historic charts or account manager.

**Genuine remaining user decision:** whether an explicitly opt-in internal Grok billing reader using an existing native login is acceptable if no safe native read-only RPC is available. Establish technical feasibility and provider permission first; user opt-in alone does not establish permission. Claude's before-first-report behavior is settled: show waiting rather than independently reading OAuth credentials.

**Unverified:** live endpoint/schema compatibility, installed provider versions and auth modes, quota fields on supported Claude versions, reliable Claude account attribution, Codex app-server coexistence/refresh behavior, and a non-mutating Grok billing access route. Superset's Grok percent/reset mapping is only source-attested and synthetic-test-covered; it does not prove 5h/7d or a stable third-party service contract. Official provider permission for direct internal OAuth/backend reads and a safe polling rate remain unestablished. No private calls or app execution were used to resolve them.

Before an eventual feature is accepted, meaningful unit tests must cover window semantics, source/profile fencing, zero versus unknown, independent windows, resets, freshness, and sanitized failures. Integration tests must drive the real reporting/collector-to-server-to-Dashboard path with isolated sockets/config and synthetic public-schema responses. Cover reattach/replacement, late responses, fifty-session deduplication, blocked collectors without PTY/input delay, and last-row scrolling/hit-testing after quota space is reserved. Native CUA on the reviewed checkout must verify narrow/short/hidden sidebar, real input, details access, visible stale/error states, screenshots and accessibility text; follow [testing](docs/development/testing.md) and [CUA](docs/testing-computer-use.md). Any authorized live provider validation is a separate permission and evidence step.

This research does not claim those future acceptance checks passed. Source/document inspection and Markdown/local-link checks are the verification performed here; app execution, Superset tests, private endpoint probes and CUA were not run. Only this research document is an intended workspace change. Stop at the brainstorming approval gate.

## Pinned Superset source references

All Superset links below are fixed to the inspected commit, not mutable `main`.

[SS1]: https://github.com/superset-sh/superset/blob/149e20ef52ffda02859cc0eeba60cb3f8fb01889/apps/desktop/src/renderer/routes/_authenticated/_dashboard/components/DashboardSidebar/components/DashboardSidebarHeader/DashboardSidebarHeader.tsx#L253-L282
[SS2]: https://github.com/superset-sh/superset/blob/149e20ef52ffda02859cc0eeba60cb3f8fb01889/apps/desktop/src/renderer/routes/_authenticated/settings/usage/utils/usageLastSection/usageLastSection.ts#L1-L30
[SS3]: https://github.com/superset-sh/superset/blob/149e20ef52ffda02859cc0eeba60cb3f8fb01889/apps/desktop/src/renderer/routes/_authenticated/settings/usage/page.tsx#L1-L30
[SS4]: https://github.com/superset-sh/superset/blob/149e20ef52ffda02859cc0eeba60cb3f8fb01889/apps/desktop/src/renderer/hooks/host-service/useHostUsageQuota/useHostUsageQuota.ts#L11-L47
[SS5]: https://github.com/superset-sh/superset/blob/149e20ef52ffda02859cc0eeba60cb3f8fb01889/apps/desktop/src/renderer/lib/host-service-client.ts#L20-L57
[SS6]: https://github.com/superset-sh/superset/blob/149e20ef52ffda02859cc0eeba60cb3f8fb01889/packages/host-service/src/trpc/router/usage/usage.ts#L42-L412
[SS7]: https://github.com/superset-sh/superset/blob/149e20ef52ffda02859cc0eeba60cb3f8fb01889/apps/desktop/src/renderer/routes/_authenticated/settings/usage/components/UsageView/UsageView.tsx#L55-L715
[SS8]: https://github.com/superset-sh/superset/blob/149e20ef52ffda02859cc0eeba60cb3f8fb01889/apps/desktop/src/renderer/routes/_authenticated/settings/usage/components/UsageView/utils/visibleQuotaAgents/visibleQuotaAgents.ts#L1-L35
[SS9]: https://github.com/superset-sh/superset/blob/149e20ef52ffda02859cc0eeba60cb3f8fb01889/apps/desktop/src/renderer/routes/_authenticated/_dashboard/v2-workspace/$workspaceId/hooks/usePaneRegistry/components/TerminalPane/components/TerminalPaneHeaderExtras/components/TerminalAccountUsage/TerminalAccountUsage.tsx#L25-L219
[SS10]: https://github.com/superset-sh/superset/blob/149e20ef52ffda02859cc0eeba60cb3f8fb01889/packages/host-service/src/trpc/router/usage/claude.ts#L1-L593
[SS11]: https://github.com/superset-sh/superset/blob/149e20ef52ffda02859cc0eeba60cb3f8fb01889/packages/host-service/src/trpc/router/usage/codex.ts#L1-L244
[SS12]: https://github.com/superset-sh/superset/blob/149e20ef52ffda02859cc0eeba60cb3f8fb01889/packages/host-service/src/trpc/router/usage/grok-quota.ts#L1-L263
[SS13]: https://github.com/superset-sh/superset/blob/149e20ef52ffda02859cc0eeba60cb3f8fb01889/packages/host-service/src/trpc/router/usage/agy-quota.ts#L1-L173
[SS14]: https://github.com/superset-sh/superset/blob/149e20ef52ffda02859cc0eeba60cb3f8fb01889/packages/host-service/src/trpc/router/usage/opencode-quota.ts#L1-L178
[SS15]: https://github.com/superset-sh/superset/blob/149e20ef52ffda02859cc0eeba60cb3f8fb01889/packages/host-service/src/trpc/router/usage/profiles.ts#L1-L327
[SS16]: https://github.com/superset-sh/superset/blob/149e20ef52ffda02859cc0eeba60cb3f8fb01889/apps/desktop/src/renderer/routes/_authenticated/settings/usage/components/UsageView/components/AddAccountDialog/AddAccountDialog.tsx#L80-L180
[SS17]: https://github.com/superset-sh/superset/blob/149e20ef52ffda02859cc0eeba60cb3f8fb01889/apps/desktop/src/renderer/providers/ElectronTRPCProvider/ElectronTRPCProvider.tsx#L26-L138
[SS18]: https://github.com/superset-sh/superset/blob/149e20ef52ffda02859cc0eeba60cb3f8fb01889/packages/host-service/src/trpc/index.ts#L110-L188
[SS19]: https://github.com/superset-sh/superset/blob/149e20ef52ffda02859cc0eeba60cb3f8fb01889/apps/desktop/src/renderer/utils/usage/formatResetIn/formatResetIn.ts#L1-L68
[SS20]: https://github.com/superset-sh/superset/blob/149e20ef52ffda02859cc0eeba60cb3f8fb01889/packages/host-service/src/trpc/router/usage/grok-quota.test.ts#L20-L38
[SS21]: https://github.com/superset-sh/superset/blob/149e20ef52ffda02859cc0eeba60cb3f8fb01889/apps/desktop/src/renderer/routes/_authenticated/settings/usage/components/UsageHistorySection/UsageHistorySection.tsx#L26-L191
[SS22]: https://github.com/superset-sh/superset/blob/149e20ef52ffda02859cc0eeba60cb3f8fb01889/packages/host-service/src/trpc/router/usage/history/entries.ts#L32-L183
[SS23]: https://github.com/superset-sh/superset/blob/149e20ef52ffda02859cc0eeba60cb3f8fb01889/apps/desktop/src/renderer/routes/_authenticated/settings/usage/components/LeaderboardCard/LeaderboardCard.tsx#L18-L105
[SS24]: https://github.com/superset-sh/superset/blob/149e20ef52ffda02859cc0eeba60cb3f8fb01889/packages/host-service/src/trpc/router/usage/history/cursor.ts#L1-L260
[SS25]: https://github.com/superset-sh/superset/blob/149e20ef52ffda02859cc0eeba60cb3f8fb01889/packages/host-service/src/trpc/router/usage/history/aggregate.ts#L236-L265
[SS26]: https://github.com/superset-sh/superset/blob/149e20ef52ffda02859cc0eeba60cb3f8fb01889/packages/host-service/src/trpc/router/usage/history/pricing.ts#L145-L232
[SS27]: https://github.com/superset-sh/superset/blob/149e20ef52ffda02859cc0eeba60cb3f8fb01889/apps/desktop/src/renderer/routes/_authenticated/hooks/useLeaderboardOptIn/useLeaderboardOptIn.ts#L16-L121
[SS28]: https://github.com/superset-sh/superset/blob/149e20ef52ffda02859cc0eeba60cb3f8fb01889/apps/desktop/src/renderer/routes/_authenticated/_dashboard/components/TopBar/components/ResourceConsumption/hooks/useResourceSnapshot/useResourceSnapshot.ts#L55-L146
[SS29]: https://github.com/superset-sh/superset/blob/149e20ef52ffda02859cc0eeba60cb3f8fb01889/apps/desktop/src/renderer/routes/_authenticated/settings/usage/hooks/useHostUsageHistory/useHostUsageHistory.ts#L1-L36
[SS30]: https://github.com/superset-sh/superset/blob/149e20ef52ffda02859cc0eeba60cb3f8fb01889/apps/desktop/src/renderer/lib/host-service-auth.ts#L1-L61
[SS31]: https://github.com/superset-sh/superset/blob/149e20ef52ffda02859cc0eeba60cb3f8fb01889/apps/desktop/src/renderer/routes/_authenticated/settings/usage/components/UsageSidebarToggle/UsageSidebarToggle.tsx#L1-L57
[SS32]: https://github.com/superset-sh/superset/blob/149e20ef52ffda02859cc0eeba60cb3f8fb01889/apps/desktop/src/lib/trpc/routers/settings/index.ts#L1151-L1175
[SS33]: https://github.com/superset-sh/superset/blob/149e20ef52ffda02859cc0eeba60cb3f8fb01889/apps/desktop/src/renderer/routes/_authenticated/_dashboard/v2-workspace/$workspaceId/hooks/usePaneRegistry/components/TerminalPane/components/TerminalPaneHeaderExtras/components/TerminalAccountUsage/utils/getAccountUsageState/getAccountUsageState.ts#L10-L66

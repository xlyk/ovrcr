# Reliable quota startup (item 3 of the usage-defaults plan)

Decided with Kyle on 2026-10-01 over two grilling rounds. Builds on item 1 (`quota.enabled` is a live consent setting with an off-state explanation) and item 2 (the Server write request). Vocabulary in `CONTEXT.md`: Allowance, Quota reason, Next check, Consent setting.

## Problem

Verified on main `cc2ac8a`: a fresh launch showed no usage because native collection is a default-off consent setting with no visible reason. Before the first read an enabled provider shows Unavailable (the snapshot default). A missing executable, a timeout, a dropped pipe and an HTTP 5xx all read Unavailable. Backoff after failures reaches 40 minutes with nothing on screen. Claude shows nothing until a managed Claude session answers.

## Decisions

1. No disk cache. The recorded decision in `provider-usage-spec.md` stands: no durable quota store, and a Server restart does not resurrect quota. Revisit only if item 4 ends without independent Claude retrieval. Last-good windows stay in Server memory across Dashboard detach and reattach, as today.
2. `QuotaState` gains `Disabled` (the consent setting is off) and `Checking` (enabled, first read in flight). `ProviderQuota` gains `reason: Option<String>`, OVRCR's own one-line classification (for example "codex not found on PATH", "timed out after 20 s", "exited with status 1", "HTTP 503"), never a response body, account value or credential; and `next_check_unix_ms`. Unavailable stays the state for transient and process failures; the reason tells them apart.
3. The Codex and Grok workers keep their shape: the enabled-live hook from item 1, immediate read on attach, five-minute cadence, one in-flight read per provider, version minimums unchanged (0.155.1, 1.0.40).
4. Backoff: transient failures (timeout, dropped pipe, EOF, HTTP 5xx) retry after 1, 2, 5 then 10 minutes, capped at 10. Deterministic failures (executable not found, unsupported version, team or non-user context, not signed in) go straight to the 10-minute cap. A provider's Retry-After always wins. A manual refresh or a change to any `quota.*` setting marks the worker due now.
5. `Request::RefreshQuota { provider: Option<QuotaProvider> }` marks the worker due now. One in-flight read per provider; a 30-second cooldown between manual refreshes, enforced in the Server, refused with the remaining cooldown; a manual refresh skips a failure backoff but never a Retry-After. Claude is not refreshable: its row says it waits for the focused managed session's first response.
6. Block (Quota left): an off provider shows "Codex/Grok usage off"; an enabled one shows Checking until its first result; a stale value shows its age ("37% left  stale 12m"); a failed row shows "retry 3m". Compact ladder when the sidebar is short: one line per provider, then a single "Quota: u" pointer line, hide only below that.
7. Details (u): the full off-state sentence with the setting path; the reason; last observation and last account check as local time plus age; next check; "subscription allowance only" once.
8. Palette: "Refresh quota" and "Enable Codex and Grok usage" (the latter through item 2's write request; until item 2 lands, the off-state sentence alone).
9. No token or spend display anywhere in the quota surfaces.
10. Claude in item 3 gets the Checking and reason treatment and the waiting sentence; independent retrieval is item 4.
11. Out of scope: independent Claude retrieval (item 4), any OAuth or credential handling, persistence, version minimum changes, lifecycle tests (item 5).

## Dependencies and assumptions

Item 1 PR 3 (quota.enabled live) for the Disabled transitions; item 2 PR A for the Enable palette command. Each protocol change bumps `PROTOCOL_VERSION`. Reasons are produced only at OVRCR's own classification points in `server/quota.rs`, each tested to carry no native body. The snapshot default for an enabled provider is Checking and for a disabled one Disabled, derived from the settings snapshot. The cooldown and the one-in-flight rule live in the Server, not the Dashboard.

## Delivery: two PRs

### PR 1: Server and protocol

Disabled and Checking; reason and next_check on ProviderQuota; the 1/2/5/10 backoff with the deterministic rule and Retry-After precedence; RefreshQuota with cooldown; Claude row wording; protocol bump; `docs/provider-quota.md`.

Acceptance: with `quota.enabled` on and a fixture native RPC, attach shows Checking then Current within one read. With it off, the snapshot carries Disabled. A missing executable yields Unavailable with reason "not found" and next check in 10 minutes. A timeout yields reason "timed out after 20 s" with next check in 1 minute, then 2, 5, 10. Retry-After overrides the ladder. A manual refresh during backoff reads at once; a second within 30 seconds is refused with the remaining cooldown. Claude shows Checking with its sentence until a managed session reports. No reason string contains any byte of a fixture response body.

### PR 2: Dashboard

Block and details display (off line, Checking, stale age, retry marker, local times, next check, the allowance sentence); the compact ladder; Refresh and Enable palette commands; `docs/dashboard.md`. CUA evidence for off, Checking, Current, stale, failed-with-retry, Refresh and Enable, and for the block at sidebar heights that give three lines, one line and zero.

## Conventions for the workers

As in item 1's spec.

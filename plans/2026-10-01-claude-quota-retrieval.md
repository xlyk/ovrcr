# Independent Claude allowance retrieval (item 4 of the usage-defaults plan)

Decided with Kyle on 2026-10-01 over three grilling rounds. Builds on item 1 (`quota.*` settings live, consent settings) and item 3 (states, reasons, next check, `RefreshQuota`). Vocabulary in `CONTEXT.md`: Allowance, Quota probe, Quota reason. Decision record: `docs/adr/0008-claude-allowance-by-probe-not-oauth.md`.

## Problem

Claude allowance reaches OVRCR only through a managed session's status line, and the status line carries `rate_limits` only after that session's first API response, so a fresh launch shows nothing for Claude until a session answers.

## Evaluation (sources read 2026-10-01)

| Route | Mechanism | Reliability | Credential ownership | Provider policy |
| --- | --- | --- | --- | --- |
| Status line (today) | `rate_limits.five_hour/seven_day/spend_limit` with `used_percentage` and `resets_at`; documented at code.claude.com/docs/en/statusline | Present only for Pro and Max logins, only after the first API response; `refreshInterval` re-runs it on a timer; a window is dropped when its `resets_at` passes | None read | Documented |
| `claude auth status --json` | Documented CLI; reports `loggedIn`, `authMethod`, `subscriptionType`, `configDirectory`, plus email and org ids | Immediate, no session | None read; identifiers not kept | Documented |
| Probe (chosen) | The unmodified `claude` binary in a PTY with OVRCR's status line, one minimal prompt | Allowance within one short run on any machine where `claude` is logged in; costs one small prompt and leaves a conversation in Claude's history | None read | Documented mechanisms; the user signs in to the unmodified binary with their own subscription, which the guidance permits. Reverses OVRCR's earlier "no prompt to obtain quota" rule for this consented path only |
| Direct read (rejected) | `GET api.anthropic.com/api/oauth/usage` with the Claude Code OAuth access token as Bearer and `anthropic-beta: oauth-2025-04-20` (oh-my-pi `packages/ai/src/usage/claude.ts`, Superset `packages/host-service/src/trpc/router/usage/claude.ts`); windows `five_hour`, `seven_day`, `seven_day_sonnet` with `utilization` and `resets_at` | Works today for those tools; the endpoint is undocumented and rate-limited; a 401/403 means the token lapsed. Superset's rule: never refresh the token, a second refresher trips token-reuse protection and signs the CLI out | Reads the token from the Keychain item `Claude Code-credentials` (keyed per `CLAUDE_CONFIG_DIR`) or `~/.claude/.credentials.json`; oh-my-pi also impersonates the Claude Code user agent | Anthropic's guidance (code.claude.com/docs/en/legal-and-compliance, "Authentication and credential use"): OAuth is intended exclusively for ordinary use of Claude Code and native Anthropic applications; developers may not collect, store or intermediate Claude.ai credentials or session tokens; enforcement may come without notice. Not certainly prohibited for a local tool on the user's own machine, not approved either |

Re-check trigger: Anthropic documents a usage API for subscribers, or changes the credential guidance.

## Decisions

1. Documented-only improvements, shipped first: the generated OVRCR status line gains `refreshInterval` 60 s so an idle managed session keeps `rate_limits` current. The Server reads `claude auth status --json` for a credential-free state: `loggedIn` false gives NotSignedIn with the reason "run claude auth login"; `authMethod` other than `claude.ai` gives Unsupported with "API-key logins have no subscription allowance". No identifier from that output is kept.
2. Consent setting `quota.claude.probe`, off by default. Description: "Starts a hidden Claude Code run with a one-word prompt and reads its status line when no managed Claude session has reported recently; each probe spends a small amount of your allowance and leaves a conversation in Claude's history; OVRCR trusts its own empty probe directory for this."
3. When on, a probe runs on attach when the Claude reading is stale, on manual refresh (item 3's cooldown applies), and every 30 minutes while a Dashboard is attached and no managed Claude session has reported inside the stale boundary. Never while no Dashboard is attached. Never two at a time.
4. Invocation: the unmodified `claude` binary in a PTY, `--model haiku`, `--tools ""`, `--permission-mode dontAsk`, `--settings` carrying only OVRCR's status line command, the prompt "Reply with OK.", in an OVRCR-owned empty directory under the instance directory (recreated empty if missing), with the reporter environment carrying a probe identity so the callback reaches the Server. The user's own settings files are not loaded. The Server accepts Claude's folder-trust dialog for that directory once, only when the probe PTY shows the dialog, and for no other directory. The process ends after the `rate_limits` callback or after 60 s.
5. The probe is hidden: a Server-internal process, never a Session. Quota details show "source: probe; last probe <time> (<age>)". `QuotaSource` gains a `Probe` variant. A live managed Claude session's report keeps precedence.
6. Failures use item 3's states and reasons: `claude` not found; probe timed out; probe exited before reporting; no `rate_limits` in the callback (not a Pro or Max login) gives Unsupported.
7. Under every route OVRCR reads no Claude credential and never refreshes, copies, persists or logs a token; no token or account identifier appears in the snapshot, logs, reasons or captures.
8. Out of scope: any OAuth or credential read, direct endpoint calls, Codex and Grok (item 3), lifecycle tests (item 5), persistence.

## Dependencies and assumptions

Item 1 (`quota.*` live, consent settings) and item 3 PR 1 (states, reason, `RefreshQuota`). The probe and managed sessions use the same Claude login (the default `CLAUDE_CONFIG_DIR`). A probe conversation under the probe directory is acceptable.

## Delivery: two PRs

### PR 1: documented-only

`refreshInterval` in the generated status line (`ovrcr agent setup claude` round-trips it); `claude auth status --json` read with the two states; this plan file. Docs: `docs/claude-code-setup.md`, `docs/provider-quota.md`.

### PR 2: the probe

Setting, probe runner, `QuotaSource::Probe`, trust handling, cadence and refresh, ADR 0008, docs (`docs/provider-quota.md`, `docs/dashboard.md`).

Acceptance: with the setting off nothing spawns and the Claude row says it waits for a managed session. With it on and a fixture `claude` that emits a status-line callback, attach yields a Current Claude reading within one probe and the process has exited. A fixture that shows a trust dialog is answered once and the second probe sees no dialog. A fixture without `rate_limits` yields Unsupported with its reason. A 60-second silent fixture yields Unavailable "probe timed out" and the process is gone. A live managed Claude session reporting suppresses probes. The 30-minute cadence and the refresh cooldown hold under a fake clock. No token string from a fixture credentials file appears anywhere in Server output.

## Conventions for the workers

As in item 1's spec.

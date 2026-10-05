# Provider allowance in the Dashboard

The bottom-left **Quota left** block shows account allowance remaining, not a terminal's tokens, cost, or context occupancy. Press `u` in Browse, or find **Quota details** in the menu or palette. Details remain accessible when the sidebar is hidden. Use Up/Down to scroll and Escape to close.

## Sources

- **Claude:** `GET https://api.anthropic.com/api/oauth/usage` with `anthropic-beta: oauth-2025-04-20` and the Claude Code access token already in `~/.claude/.credentials.json` (`$CLAUDE_CONFIG_DIR` when set). `five_hour` and `seven_day` become 5h/7d rows. OVRCR does not refresh or rewrite that file, and it does not start a new login. A managed session's status-line `rate_limits` is only an in-session extra: the account row does not wait on it. The consented probe below remains optional.
- **Codex:** a task-owned `codex -s read-only -a never app-server`, then JSON-RPC `account/rateLimits/read`. Codex keeps the token. OVRCR does not `GET chatgpt.com/backend-api/wham/usage` and does not write `auth.json`. Window durations come from the response; primary and secondary are not assumed to mean 5h and 7d. Model-specific buckets remain in details, not general sidebar bars. The reviewed version is **0.155.1**.
- **Grok:** `GET https://cli-chat-proxy.grok.com/v1/billing?format=credits` with the existing `~/.grok/auth.json` key (`$GROK_HOME` or `quota.grok.home`) and header `x-xai-token-auth: xai-grok-cli`. Read-only: the file is not refreshed or rewritten. `x.ai/billing` on `grok agent stdio` is method-not-found and is not the path. The typed `creditUsagePercent` and weekly or monthly `currentPeriod` determine the label. Prepaid credits, on-demand spend, session tokens and cost are not general allowance. A team identifier in the native login does not by itself reject subscription allowance. Accounts without validated general subscription windows stay unknown or unsupported.

Claude's selected source follows the focused managed Claude session and is retained when focus moves elsewhere. A connected source keeps **waiting for report** until native quota arrives; ordinary metrics and resizing do not make it unavailable. Details identify its Session, run and Reporting generation. Native collectors use the configured native profile; account metadata stays private and never appears on the Dashboard wire.

### Claude sign-in state

The Server runs `claude auth status --json` (the executable named by the `agents` entry called `claude` in the settings document, else `claude` on PATH) once at start and again whenever a `quota.*` setting or that executable changes. There is no timer: after you sign in or out, the row follows on the next such change or Server start. A reply is bounded to twenty seconds and 2 MiB. Only `loggedIn` and `authMethod` are read; the email, organization and every other identifier are dropped and never stored, logged, put in a reason or sent to the Dashboard.

| `claude auth status` | Claude row |
| --- | --- |
| `loggedIn` false | not signed in, "run claude auth login" |
| signed in, `authMethod` other than `claude.ai` | unsupported, "API-key logins have no subscription allowance" |
| signed in through `claude.ai` | checking until the oauth usage read; with no credential file, waiting for a managed session's first response |
| `claude` missing, failing or unreadable | unavailable, with the reason (for example "claude not found on PATH" or "claude auth status unreadable") |

This state applies only while the row would otherwise be checking. An oauth usage reading with windows is the account row and does not wait for a managed session. A status-line `rate_limits` sample remains an in-session extra when that read has not produced windows. Waiting, unsupported, and not signed in stay when the read has not succeeded or cannot. This is not a native acceptance pass.

### Claude quota probe

`quota.claude.probe` is off by default. Turning it on lets the Server run the unmodified `claude` binary (the same executable as `claude auth status`: the `agents` entry named `claude`, else `claude` on PATH) in a PTY when no managed Claude session has reported inside the stale boundary. The probe is a Server-internal process, never a Session. It spends one small prompt and leaves a conversation in Claude's history; the setting says so.

The run uses `--model haiku`, `--tools ""`, `--permission-mode dontAsk`, and `--settings` that carry only OVRCR's status-line command. The prompt is `Reply with OK.`. The working directory is an OVRCR-owned empty directory under the instance directory, recreated empty if it is missing. The user's own settings files are not loaded. The reporter environment carries a probe identity so the status-line callback reaches the Server and is not admitted as a Session. If that PTY shows Claude's folder-trust dialog for the probe directory, the Server answers it once, and for no other directory. The process ends when the callback includes `rate_limits`, or after 60 seconds.

A probe runs on Dashboard attach when the Claude reading is stale, on manual refresh (the same 30-second cooldown as Codex and Grok), and every 30 minutes while a Dashboard stays attached and no managed Claude session has reported inside the stale boundary. It never runs while no Dashboard is attached, and never two at a time. It also waits until `claude auth status` has said an allowance can exist: not signed in, an API-key login, or a missing `claude` stays on that row and does not spend a prompt.

Details for a probe reading say `source: probe; last probe <time> (<age>)`. A live managed Claude session's report keeps precedence over it. `quota.enabled` does not gate the probe; that switch is still only Codex and Grok.

| Probe outcome | Claude row |
| --- | --- |
| status line with `rate_limits` | current, source probe |
| status line without `rate_limits` (not a Pro or Max login) | unsupported, "not a Pro or Max login" |
| no callback within 60 seconds | unavailable, "probe timed out" |
| process ended with no callback | unavailable, "probe exited before reporting" |
| `claude` missing | unavailable, "claude not found on PATH" or "claude not found at the configured path" |

## Collection

Claude's account row comes from the oauth usage reader and does not need `quota.enabled`. Codex and Grok collection is on by default while a Dashboard is attached. The readers are read-only: they do not refresh or rewrite auth files, which is why the old opt-in default is no longer required. Set `quota.enabled = false` in the settings document to turn Codex and Grok collection off. The setting remains; only the default changed. `dashboard.toml` sits beside the OVRCR `config.toml` (a `[quota]` table in `config.toml` configures nothing and `ovrcr settings` reports it as a finding). A change applies on the next settings check; a restart is not required.

```toml
[quota]
# enabled = false                 # explicit off; omit to collect while a Dashboard is attached

[quota.claude]
probe = false                     # opt in: a hidden Claude run may spend allowance

[quota.codex]
command = "codex"
# home = "/absolute/path/to/codex-profile" # native CODEX_HOME

[quota.grok]
command = "grok"
# home = "/absolute/path/to/grok-profile" # native GROK_HOME
```

Omit `home` to use the native client's existing profile. OVRCR does not provide login, extract tokens, copy credentials, or attach to a shared native client. Unsupported versions fail closed. Native collection runs only while a Dashboard is attached, with one worker per provider, a five-minute cadence, bounded replies and a twenty-second request deadline, and failure backoff (below). The deadline covers native writes and incoming notifications as well as replies. Fifty Sessions do not create fifty account readers. Every `quota.*` setting is live: a change restarts the native client and reads at once. [Native Codex read/UI acceptance](../research/codex-quota-native-acceptance-2026-10-05/README.md) verified the installed 0.155.1 CLI with an existing ChatGPT login on `05adc3d0`. The record keeps unobserved native notifications/profile switching and the combined three-provider lifecycle gate (#231) explicit.

## Interpret the display

A bar represents `100 − consumed` with fill `━` in the provider color and track `─` in SURFACE2 (no brackets); `—` is unknown, not zero or full. Exhausted and over-limit reports are distinct. Passed reset times show **reset due**; OVRCR never refills allowance locally. Unavailable or stale sources retain last-good values with a stale marker and their age since the last observation or account check, such as `37% stale 12m`. Before any source reports, Claude shows checking and Codex and Grok show checking (collection is on unless `quota.enabled` is false). **Codex · usage off** / **Grok · usage off** is the explicit off state. A failed row without values shows its next check, such as `unavailable · retry 3m`. Narrow sidebars place the window's full state below its identity. A short sidebar drops the trailing blank first, then shows one line per provider, then a single `Quota: u` line, and hides the block only below that; details stay on `u`.

Details open with "Subscription allowance only": the block and details never show tokens or spend. For each provider they give the state, the off-state sentence with its setting path or the quota reason, the source, the last observation and last account check as local time with age, and the next check as local time. Details distinguish the last changed observation from the last successful native account check. Repeated Claude callbacks cannot claim a backend check. Account changes invalidate older native generations, including A→B→A notifications received during a read. Quota snapshots are bounded, in-memory Server state and are not persisted as account history.

## States, reasons and next check

Each provider row carries a state, a **Quota reason** and a **Next check**, all set by the Server:

| State | Meaning |
| --- | --- |
| off | `quota.enabled` is explicitly false. The reason is the off-state sentence naming the setting. The default is on. |
| checking | The first read is in flight. Codex and Grok stay here until their account read returns, unless `quota.enabled` is false. Claude stays here, with the reason "waiting for a managed Claude session's first response", until the oauth usage read, a managed session, or a consented probe reports, unless `claude auth status` rules an allowance out. Waiting, unsupported, and not signed in stay as they are when that read has not succeeded or cannot. |
| current | The last read succeeded. The next check is five minutes later. |
| unavailable, not signed in, unsupported, invalid, source conflict | The last read failed. The reason says why. |

A reason is OVRCR's own one-line classification, such as "codex not found on PATH", "grok not found at the configured path", "codex version unsupported (needs 0.155.1)", "timed out after 20 s", "native pipe closed", "native client closed its output", "HTTP 503", "not signed in" or "team or non-user context unsupported". It never carries a native response body, an account value or a credential.

After a failure the next check follows a ladder:

- Transient failures (timeout, closed pipe or output, HTTP 5xx and other request failures) retry after 1, 2, 5, then 10 minutes, and stay at 10.
- Deterministic failures (executable not found, unsupported version or method, team or non-user context, not signed in) wait 10 minutes at once.
- A provider's Retry-After always wins over the ladder, bounded to between one minute and one day.

A success resets the ladder. Changing any `quota.*` setting also resets it and reads at once.

### Manual refresh

`Request::RefreshQuota { provider }` (both native providers when `provider` is none) marks the worker due now. The Server enforces it:

- One read per provider is in flight at a time; a refresh asked for during a read is answered by that read.
- Accepted refreshes are at least 30 seconds apart. A refresh inside the cooldown is refused with `Response::QuotaCooldown { remaining_ms }`.
- A refresh skips a failure backoff but never a provider's Retry-After.
- With `quota.claude.probe` off, Claude is not refreshable. With it on, a refresh runs one probe under the same cooldown, including when `provider` is Claude or omitted. A refresh while `quota.enabled` is off is still refused for Codex and Grok; an omitted provider with the probe on still runs the probe and does not start the native workers.

In the Dashboard, the palette's **Refresh quota** sends this request for both native providers and shows a cooldown refusal's remaining seconds in the footer. **Enable Codex and Grok usage**, listed while collection is off, sends `SetSetting { path: "quota.enabled", value: "true" }`; the Server validates, writes and republishes, and the Dashboard applies nothing until it does.


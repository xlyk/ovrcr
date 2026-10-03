# Provider allowance in the Dashboard

The bottom-left **Quota left** block shows account allowance remaining, not a terminal's tokens, cost, or context occupancy. Press `u` in Browse, or find **Quota details** in the menu or palette. Details remain accessible when the sidebar is hidden. Use Up/Down to scroll and Escape to close.

## Sources

- **Claude:** managed Claude Code status-line callbacks. Only native `five_hour` and `seven_day` reports become 5h/7d windows. Claude may not report quota until its first API response. OVRCR does not issue prompts to obtain it or read Claude OAuth credentials.
- **Codex:** a task-owned `codex app-server`, using native account and rate-limit reads. Window durations come from the response; primary and secondary are not assumed to mean 5h and 7d. Model-specific buckets remain in details, not general sidebar bars. The reviewed version is **0.155.1**.
- **Grok:** a task-owned standalone ACP client, using native auth metadata and typed billing. The native weekly or monthly period determines its label. Prepaid credits, on-demand spend, session tokens and cost are not general allowance. Team/non-user contexts are unsupported. The reviewed version is **1.0.40**.

Claude's selected source follows the focused managed Claude session and is retained when focus moves elsewhere. A connected source keeps **waiting for report** until native quota arrives; ordinary metrics and resizing do not make it unavailable. Details identify its Session, run and Reporting generation. Native collectors use the configured native profile; account metadata stays private and never appears on the Dashboard wire.

## Enable native collection

Claude callbacks do not need an additional switch. Codex and Grok collection is opt-in because native clients may refresh their own authentication and write their own logs. Add this to the settings document, `dashboard.toml` beside the OVRCR `config.toml` (a `[quota]` table in `config.toml` configures nothing and `ovrcr settings` reports it as a finding), then restart the Server after stopping or preserving your sessions through the normal lifecycle:

```toml
[quota]
enabled = true

[quota.codex]
command = "codex"
# home = "/absolute/path/to/codex-profile" # native CODEX_HOME

[quota.grok]
command = "grok"
# home = "/absolute/path/to/grok-profile" # native GROK_HOME
```

Omit `home` to use the native client's existing profile. OVRCR does not provide login, extract tokens, copy credentials, or attach to a shared native client. Unsupported versions fail closed. Native collection runs only while a Dashboard is attached, with one worker per provider, a five-minute cadence, bounded replies and a twenty-second request deadline, and failure backoff (below). The deadline covers native writes and incoming notifications as well as replies. Fifty Sessions do not create fifty account readers. Every `quota.*` setting is live: a change restarts the native client and reads at once.

## Interpret the display

A bar represents `100 − consumed`; `—` is unknown, not zero or full. Exhausted and over-limit reports are distinct. Passed reset times show **reset due**; OVRCR never refills allowance locally. Unavailable or stale sources retain last-good values with a stale marker and their age since the last observation or account check, such as `37% left  stale 12m`. Before any source reports, Claude shows checking and each native provider shows checking when `quota.enabled` is on, or **Codex usage off** / **Grok usage off** when it is not. A failed row without values shows its next check, such as `unavailable  retry 3m`. Narrow sidebars place the window's full state below its identity. A short sidebar shows one line per provider, then a single `Quota: u` line, and hides the block only below that; details stay on `u`.

Details open with "Subscription allowance only": the block and details never show tokens or spend. For each provider they give the state, the off-state sentence with its setting path or the quota reason, the source, the last observation and last account check as local time with age, and the next check as local time. Details distinguish the last changed observation from the last successful native account check. Repeated Claude callbacks cannot claim a backend check. Account changes invalidate older native generations, including A→B→A notifications received during a read. Quota snapshots are bounded, in-memory Server state and are not persisted as account history.

## States, reasons and next check

Each provider row carries a state, a **Quota reason** and a **Next check**, all set by the Server:

| State | Meaning |
| --- | --- |
| off | `quota.enabled` is off. The reason is the off-state sentence naming the setting. |
| checking | Enabled, and the first read is in flight. Claude stays here, with the reason "waiting for a managed Claude session's first response", until a managed session reports. |
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
- Claude is not refreshable, and a refresh while `quota.enabled` is off is refused with the off-state sentence.

In the Dashboard, the palette's **Refresh quota** sends this request for both native providers and shows a cooldown refusal's remaining seconds in the footer. **Enable Codex and Grok usage**, listed while collection is off, sends `SetSetting { path: "quota.enabled", value: "true" }`; the Server validates, writes and republishes, and the Dashboard applies nothing until it does.


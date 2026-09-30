# Provider allowance in the Dashboard

The bottom-left **Quota left** block shows account allowance remaining, not a terminal's tokens, cost, or context occupancy. Press `u` in Browse, or find **Quota details** in the menu or palette. Details remain accessible when the sidebar is hidden. Use Up/Down to scroll and Escape to close.

## Sources

- **Claude:** managed Claude Code status-line callbacks. Only native `five_hour` and `seven_day` reports become 5h/7d windows. Claude may not report quota until its first API response. OVRCR does not issue prompts to obtain it or read Claude OAuth credentials.
- **Codex:** a task-owned `codex app-server`, using native account and rate-limit reads. Window durations come from the response; primary and secondary are not assumed to mean 5h and 7d. Model-specific buckets remain in details, not general sidebar bars. The reviewed version is **0.155.1**.
- **Grok:** a task-owned standalone ACP client, using native auth metadata and typed billing. The native weekly or monthly period determines its label. Prepaid credits, on-demand spend, session tokens and cost are not general allowance. Team/non-user contexts are unsupported. The reviewed version is **1.0.40**.

Claude's selected source follows the focused managed Claude session and is retained when focus moves elsewhere. Details identify its Session, run and Reporting generation. Native collectors use the configured native profile; account metadata stays private and never appears on the Dashboard wire.

## Enable native collection

Claude callbacks do not need an additional switch. Codex and Grok collection is opt-in because native clients may refresh their own authentication and write their own logs. Add this to the OVRCR `config.toml`, then restart the Server after stopping or preserving your sessions through the normal lifecycle:

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

Omit `home` to use the native client's existing profile. OVRCR does not provide login, extract tokens, copy credentials, or attach to a shared native client. Unsupported versions fail closed. Native collection runs only while a Dashboard is attached, with one worker per provider, a five-minute cadence, bounded replies/deadlines, and failure backoff. Fifty Sessions do not create fifty account readers.

## Interpret the display

A bar represents `100 − consumed`; `—` is unknown, not zero or full. Exhausted and over-limit reports are distinct. Passed reset times show **reset due**; OVRCR never refills allowance locally. Unavailable or stale sources retain last-good values with a stale marker.

Details distinguish the last changed observation from the last successful native account check. Repeated Claude callbacks cannot claim a backend check. Account changes invalidate older native generations, including A→B→A notifications received during a read. Quota snapshots are bounded, in-memory Server state and are not persisted as account history.

# Antigravity quota display and status-line supplement acceptance

Native macOS acceptance exercised `target/OVRCR GUI.app` built from the reviewed
`feat/gemini-quota-484` checkout (base `1f17385b3` on `feat/gemini-quota`) against
task-owned `OVRCR_HOME` and `OVRCR_SOCKET` instances, alongside installed
Antigravity CLI (`/opt/homebrew/bin/agy`, version `1.5.9`).

## Structural `/usage` gate

In an isolated scratch directory with `AGY_CLI_DISABLE_AUTO_UPDATE=true` and
failing `open`/`xdg-open`/`BROWSER` stubs, `/opt/homebrew/bin/agy -p /usage
--output-format json --settings {}` exited `0` with `status: "SUCCESS"`,
`command.name: "usage"`, `group_count: 2` (`gemini-weekly`, `gemini-5h`,
`3p-weekly`, `3p-5h`), `num_turns: 0`, `total_tokens: 0`, and untouched browser
guards ([native-probe.json](native-probe.json)). No raw `/usage` body, email,
plan description, URL, or token was retained.

## Native macOS Dashboard checks

| Check | Evidence |
| --- | --- |
| Held `/usage`, status-line supplement, and live terminal input | While the `agy` `/usage` collector was held in flight, a managed `agy` session published foreground `agy-statusline` buckets (`88%` `5h`, `64%` `7d`) and live terminal input produced a separate `CUA_AGY_QUOTA_INPUT_OK` line in [01-held-supplement-and-input.jpg](01-held-supplement-and-input.jpg). |
| Authoritative `/usage` handoff and `TEAL` sidebar bars | Releasing the held `/usage` read replaced the session supplement with authoritative `TEAL` `Agy` `5h` (`72%`) and `7d` (`41%`) bars under `QUOTA LEFT` in [02-usage-bars-teal.jpg](02-usage-bars-teal.jpg). |
| **Quota details** (`u`, mouse click, palette) | Pressing `u` ([03-details-key-u.jpg](03-details-key-u.jpg)), clicking `QUOTA LEFT` ([04-details-mouse-click.jpg](04-details-mouse-click.jpg)), and running `Quota details` from the command palette ([05-palette-quota-details.jpg](05-palette-quota-details.jpg)) showed `Agy · up to date`, `5-hour  72% left`, `weekly  41% left`, `5-hour (3p-5h)  85% left`, `weekly (3p-weekly)  60% left`, and `as of … · next check …`. |
| Narrow window and sidebar toggle (`b`) | Narrowing the window kept `QUOTA LEFT` and both `Agy` bars readable ([06-narrow-sidebar.jpg](06-narrow-sidebar.jpg)); `b` hid and restored the sidebar and quota block ([07-sidebar-hidden.jpg](07-sidebar-hidden.jpg)). |
| Vertical shrinking ladder (`full -> 5-row compact -> Quota: u`) | Shortening to 22 rows compacted `QUOTA LEFT` into the 5-row summary (`Agy 5h 72%`, [08-compact-5row.jpg](08-compact-5row.jpg)), and a 9-row isolated nested Dashboard compacted further to `Quota: u` ([09-nested-quota-u.jpg](09-nested-quota-u.jpg)). |
| Installed `/opt/homebrew/bin/agy` (`1.5.9`) live run | With `quota.enabled = true` in an isolated `OVRCR_HOME`, installed `agy` `1.5.9` rendered live `TEAL` `Agy` `5h` and `7d` sidebar bars ([10-live-native-bars.jpg](10-live-native-bars.jpg)) and all four live Gemini + `3p` buckets in **Quota details** ([11-live-native-details.jpg](11-live-native-details.jpg)) with zero personal identifiers on screen or in AX text. |

Each JPEG has an adjacent accessibility snapshot with the same basename.
Closing each native window exited the launcher cleanly (`0`), removed its
disposable instance root and socket, and left every recorded session and helper
process group (`15` in the controlled pass, `14` in the live native pass)
raising `ProcessLookupError` on `os.kill(-pgid, 0)` ([native-probe.json](native-probe.json)).

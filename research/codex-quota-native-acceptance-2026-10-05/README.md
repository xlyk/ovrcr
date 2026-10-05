# Codex native quota acceptance — 2026-10-05

The installed **Codex CLI 0.155.1**, logged in through **ChatGPT**, delivered real account allowance through OVRCR's production collector, Server, socket, sidebar and details. The genuine account reported **80% remaining in a seven-day primary window**, resetting at `2026-10-10T04:26:52Z`; OVRCR did not invent a five-hour window.

Tested implementation: [`05adc3d0cfdb23eacb31e3b6ac50db652d8754ce`](https://github.com/xlyk/ovrcr/commit/05adc3d0cfdb23eacb31e3b6ac50db652d8754ce). This PR records an acceptance run; it changes no runtime behavior. [#229](https://github.com/xlyk/ovrcr/issues/229) and the combined lifecycle gate [#231](https://github.com/xlyk/ovrcr/issues/231) remain open.

## Native account and desktop evidence

The user authorized this existing login for read-only quota acceptance. OVRCR launched its own `codex -s read-only -a never app-server` in disposable empty workspaces, leaving existing Codex clients running. No model turn, conversation, credential copy, login or shared-client takeover was requested. Only the supported version and this ChatGPT auth mode received live acceptance.

All screenshots below come from the actual native app at the tested implementation revision. Each has matching accessibility text, with trailing terminal-cell padding and blank EOF lines trimmed from the published text. The project/session rows are the disposable GUI helper's fixtures.

| Check | Observed result | Screenshot | Accessibility |
| --- | --- | --- | --- |
| Real allowance/details | 7d primary 80%, actual reset, source generation and check times | [05](05-live-details.jpg) | [05](05-live-details.txt) |
| Real terminal input | Separate output `CUA_CODEX_LIVE_OK`, distinct from command echo | [06](06-live-input.jpg) | [06](06-live-input.txt) |
| Unchanged successful refresh | Observation stays `00:23:56`; account check advances to `00:30:23`; next check is five minutes later | [09](09-live-refreshed-details.jpg) | [09](09-live-refreshed-details.txt) |
| Narrow sidebar | Full provider/window identity and 80% on readable separate rows | [10](10-live-narrow-sidebar.jpg) | [10](10-live-narrow-sidebar.txt) |
| Hidden sidebar | `u` still opens real source, allowance and reset details | [11](11-live-hidden-sidebar-details.jpg) | [11](11-live-hidden-sidebar-details.txt) |
| Short native window | Allowance and input remain visible; details fit | [12](12-live-short-window.jpg), [14](14-live-short-details.jpg) | [12](12-live-short-window.txt), [14](14-live-short-details.txt) |
| Detach/reattach | Same shell and cached source/value/age survive; interrupted reader shows `read interrupted` and stale last-good 80% | [15](15-live-detached.jpg), [16](16-live-reattached-details.jpg) | [15](15-live-detached.txt), [16](16-live-reattached-details.txt) |
| Recovery/cooldown | Refresh restores reported 80% and advances account check; immediate repeat reports 30-second cooldown | [18](18-live-cooldown.jpg), [19](19-live-recovered-details.jpg) | [18](18-live-cooldown.txt), [19](19-live-recovered-details.txt) |

Details display times in the host's local timezone; reset timestamps are UTC. The observation records changed allowance, while the account-check time advances on an unchanged successful native read.

## Synthetic external RPC through the native app

These cases used the existing ignored [`native_quota_rpc_fixture`](https://github.com/xlyk/ovrcr/blob/05adc3d0cfdb23eacb31e3b6ac50db652d8754ce/tests/provider_quota.rs#L16) with an empty fake native profile. The production Server, socket, Dashboard and terminal remained real. The values below are fixture literals, not measurements of the user's account.

| Check | Observed result | Screenshot | Accessibility |
| --- | --- | --- | --- |
| Request deadline | Flood of unrelated notifications ends with `timed out after 20 s`, truthful unavailable state and no fabricated allowance | [27](27-fixture-timeout-details.jpg) | [27](27-fixture-timeout-details.txt) |
| Input during a pending read | A new flood read starts (method count 2→3); separate `CUA_CODEX_BLOCKED_REFRESH_OK` output appears within 3286 ms, before the request deadline | [28](28-fixture-blocked-refresh-input.jpg) | [28](28-fixture-blocked-refresh-input.txt), [timing](28-fixture-blocked-refresh-timing.json), [method log](44-fixture-native-methods.txt) |
| Independent windows | Actual 5h/7d durations, primary/secondary IDs, 33%/69% remaining and independent resets reach sidebar/details | [30](30-fixture-actual-window-details.jpg) | [30](30-fixture-actual-window-details.txt) |
| Nine rows | Three provider summaries; details scroll to both window rows | [32](32-fixture-nine-row-dashboard-ready.jpg), [34](34-fixture-nine-row-secondary.jpg) | [32](32-fixture-nine-row-dashboard-ready.txt), [34](34-fixture-nine-row-secondary.txt) |
| Seven rows | Single `Quota: u` pointer; details preserve both windows and stale age | [35](35-fixture-seven-row-dashboard.jpg), [37](37-fixture-seven-row-windows.jpg) | [35](35-fixture-seven-row-dashboard.txt), [37](37-fixture-seven-row-windows.txt) |
| Five rows | Quota block hidden; `u` details still scroll to primary 33% and secondary 69% | [38](38-fixture-five-row-hidden-quota.jpg), [41](41-fixture-five-row-primary.jpg), [42](42-fixture-five-row-secondary.jpg) | [38](38-fixture-five-row-hidden-quota.txt), [41](41-fixture-five-row-primary.txt), [42](42-fixture-five-row-secondary.txt) |
| Nested shutdown | Explicit private Server shutdown produces separate `CUA_CODEX_LADDER_DONE` output | [43](43-fixture-ladder-exit.jpg) | [43](43-fixture-ladder-exit.txt) |

The native helper has a minimum window height. The 9/7/5-row checks ran a second isolated Dashboard inside its fixture terminal after `stty rows N`, as described in [the computer-use guide](../../docs/testing-computer-use.md). The nested empty-tree fixture also reports the expected `Claude reporting hooks missing` warning for its empty private Claude profile; real Claude setup was not changed.

## Regression and cleanup verification

The focused command was `cargo test --locked --offline -p ovrcr --test provider_quota --test quota_schema -- --test-threads=1`, with `GROK_HOME`, `CODEX_HOME` and `CLAUDE_CONFIG_DIR` pointing at separate empty disposable directories. [The successful log](21-isolated-focused-tests.log) records **20 provider integration tests and four schema tests passed**, zero failures and one ignored helper. Compilation paths in the published logs are normalized to `<checkout>`.

[The first attempt](13-focused-tests.log) failed `missing_executable_is_not_found_with_next_check_at_the_cap` (19 passed, one failed, one ignored), so its schema binary did not run. That ordinary `Live::idle()` fixture inherits native homes and expects Grok `not signed in`, while the real Grok login is present. Empty provider homes resolved this isolation error without changing assertions or production code. Preserve the failure alongside the passing run.

The passing tests cover literal independent window values, model bucket identity, A→B→A fencing, one account read across fifty fixture Terminal sessions, reattachment without duplicate reads, consent changes, request deadlines, Retry-After/cooldown and percentage/wire validation. The existing all-feature workspace run from [PR #281](https://github.com/xlyk/ovrcr/pull/281) recorded 1397 passed/zero failed/24 ignored across 37 binaries, strict lint/format and 47 Node tests. That result was reused because `git diff --exit-code 1eddacfab20ebd01763e17d3b1ed43ea362be93b 05adc3d0cfdb23eacb31e3b6ac50db652d8754ce` confirms identical tracked trees; it is not a new broad test run for these evidence files.

[The sanitized verification summary](verification.json) records both GUI launchers exiting zero; all **36 recorded task-owned process groups**, GUI roots/sockets, nested socket and recorded native workspaces absent; temporary controls removed; the auth file unchanged at mode 0600; and all **five original Codex processes preserved**. Credential hashes, account identifiers, provider response bodies and the inventory of pre-existing processes are excluded from this publication.

## Remaining gates

- This run observes live account reads, unchanged refresh, coexistence and native UI. It did not observe a genuine `account/rateLimits/updated` notification or live account/profile switching, and does not certify every #229 acceptance checkbox.
- #231's three-provider native matrix, fifty mixed live sessions, replacement/source changes, backpressure/failure and restart remain unverified. The passing fifty-session regression uses Terminal sessions and fake external providers.
- Other Codex versions/auth modes, native Linux, paid model work and capacity measurements are outside this record. PR #280 was still open at the acceptance closeout and was not merged by this run.

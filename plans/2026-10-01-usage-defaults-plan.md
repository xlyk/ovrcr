# Usage defaults plan: five items, eleven PRs

Planned with Kyle on 2026-10-01 in fourteen grilling rounds, one item at a time. A fresh OVRCR build showed no usage for Claude, Codex or Grok, for at least the second time because of default-off settings nobody could see. The plan gives OVRCR one settings document with visible effective values and findings, an editor, honest quota states, a documented route to Claude allowance, and tests that run against production defaults.

| Item | Spec | Decision records |
| --- | --- | --- |
| 1 Shared configuration and diagnostics | `plans/2026-10-01-shared-settings.md` | ADR 0007; CONTEXT terms Setting, Settings document, Instance identity, Effective value, Source, Finding, Consent setting, Settings snapshot |
| 2 Settings editor | `plans/2026-10-01-settings-editor.md` | ADR 0007 (single writer); CONTEXT term Setting path |
| 3 Reliable quota startup | `plans/2026-10-01-quota-startup.md` | CONTEXT terms Allowance, Quota reason, Next check |
| 4 Independent Claude allowance | `plans/2026-10-01-claude-quota-retrieval.md` | ADR 0008; CONTEXT term Quota probe |
| 5 Fresh-install and lifecycle tests | `plans/2026-10-01-settings-lifecycle-tests.md` | none |

A server and Dashboard logs viewer (for diagnosing features such as automatic titles) is a separate follow-on topic and is not part of these items.

## Order

Wave 1, in flight (stacked branches): item 1 PR 1 loader (`feature/settings-loader`), PR 2 snapshot (`feature/settings-snapshot`), PR 3 watcher and live settings (`feature/settings-live`).

Wave 2, after item 1 merges: item 5 PR 1 (matrix and fixtures), item 2 PR A (Server write request, writer move, `ovrcr settings set` and `reset`), item 3 PR 1 (states, reasons, backoff, `RefreshQuota`).

Wave 3: item 2 PR B (editor), item 3 PR 2 (quota display, Refresh and Enable commands), item 4 PR 1 (`refreshInterval`, `claude auth status`).

Wave 4: item 4 PR 2 (the probe), item 5 PR 2 (the seven real-process cases).

Dependencies: item 5 PR 1 and item 2 PR A need item 1 PR 1 (PR A also PR 2 for the toggle reroute); item 3 PR 1 needs item 1 PR 3; item 2 PR B needs item 1 PR 2 and item 2 PR A; item 3 PR 2 needs item 3 PR 1 and item 2 PR A; item 4 PR 1 needs item 3 PR 1; item 4 PR 2 needs item 4 PR 1 and item 1; item 5 PR 2 needs item 2 PR A and item 3 PR 1.

Each wave starts only when Kyle says so, as new ovrcr workspaces with Claude Opus 5.5 sessions, the way wave 1 started. Merging needs Kyle's authorization.

## Rules every PR shares

One settings document; the Server is the only reader and the only writer; a bad value defaults only itself and is reported; consent settings stay off and say what to set; no credential is read, refreshed, copied, persisted or logged; counts and states on screen are facts, never guesses. Each protocol change bumps `PROTOCOL_VERSION`. TUI changes carry CUA evidence. Docs change in the same PR as the behaviour.

# Shared settings and diagnostics (item 1 of the usage-defaults plan)

Decided with Kyle on 2026-10-01 over four grilling rounds. This is the approved spec for item 1. Items 2 to 5 (settings editor, reliable quota startup, Claude quota retrieval, lifecycle tests) are planned separately and build on this.

Vocabulary is in `CONTEXT.md`: Setting, Settings document, Instance identity, Effective value, Source, Finding, Consent setting, Settings snapshot. The decision record is `docs/adr/0007-one-settings-document-read-by-the-server.md`.

## Problem

Verified on main `cc2ac8a`:

- Four independent parsers read user settings. The Dashboard parses `dashboard.toml` into one typed struct (`crates/ovrcr-tui/src/dashboard/settings.rs`), so one wrong type on any key throws every setting to its default with a footer error. The Server reads the same file twice as untyped TOML (`server/title.rs` for `title_model`, `server/preferences.rs` for `automatic_local_terminals`) and silently defaults on every problem.
- `[quota]` is read from `config.toml`, not `dashboard.toml` (`server/quota.rs` reads the registry path) with `deny_unknown_fields` and `.ok()`, so one typo under `[quota.*]` silently disables Codex and Grok collection. `docs/dashboard.md` says never put settings in `config.toml`; `docs/provider-quota.md` puts `[quota]` there.
- Nothing on the wire carries settings, so the Dashboard cannot show what the Server loaded. The installed service receives only `OVRCR_CONFIG` and `OVRCR_SOCKET` (`src/service.rs`), so a shell export of `OVRCR_DASHBOARD_CONFIG` makes the client and the service read different files.
- Unknown keys and unknown enum spellings are ignored everywhere.

## Decisions

1. One settings document, `dashboard.toml` beside the instance identity. `config.toml` is the instance identity only. A `[quota]` table found in `config.toml` yields a finding and configures nothing. No compat read.
2. One loader in `ovrcr-runtime` returns typed settings plus, per setting, its effective value, its source (default or document), and findings. There is no per-setting environment layer; env vars choose file paths only.
3. The Server is the only reader. It loads at startup, stats the document every 2 seconds on an existing Server tick, reloads when length or mtime changed, and republishes only when the effective reading or the findings changed.
4. The Server publishes a settings snapshot to the Dashboard at hello and on every change: per setting the effective value and source, plus findings (key path, message, line when known), the document path, and the read time. The Dashboard has no parser of its own.
5. Failure semantics: a bad value defaults only its own setting and yields a finding. An unknown key yields a finding and is ignored. An unknown enum spelling is a finding, not a silent default. Only an unparseable document falls back to all defaults, with a document-level finding. No settings mistake prevents the Server from starting or a Dashboard from attaching.
6. Every setting is live. There is no reload-policy attribute. `title_model` takes effect on the title worker's next tick. For `[quota]`, item 1 makes only `enabled` live: a worker that finds `enabled = false` waits for the next settings change instead of exiting, and a running worker checks `enabled` each cycle and stops when it turns off. `command` and `home` changes while running belong to item 3.
7. Consent settings stay off: `quota.enabled` (the native CLI may refresh its own auth and write logs), `title_model` (paid calls), `desktop_notifications` (OS permission). Each has an off-state explanation defined in the settings model, for example "Codex/Grok usage off: set `quota.enabled = true` in dashboard.toml". Item 3 displays the quota one in the Quota left block. The titles one appears only in the Settings view and the CLI. `ready_sound` and `automatic_local_terminals` keep today's defaults.
8. `quota.*.command` and `quota.*.home` are published like any setting.
9. Diagnostics: `ovrcr settings` prints the local reading (resolved document path, each setting with owner, effective value and source, then findings) and takes `--json`; exit 0 even with findings. The Dashboard gets a read-only Settings popup opened from the menu and the palette, no new key, rows in the order of `docs/dashboard.md`, findings at the top. One footer line at attach and whenever the finding count changes, for example "2 settings findings; see Settings".
10. Until item 2 moves writes through the Server, the Dashboard's existing writer (N, ready sound, L, launch choices) writes to the document path the Server published. The Dashboard stops resolving `OVRCR_DASHBOARD_CONFIG` itself. A toggle applies at once, as today; the Server's watcher re-reads and the snapshot confirms.
11. Adding a `ServerMessage` variant bumps `PROTOCOL_VERSION` and refreshes the wire snapshot fixture. An old client sees today's protocol-mismatch message.
12. The GUI helper's `OVRCR_GUI_QUOTA_CONFIG` fragment is appended to the demo's `dashboard.toml` instead of its `config.toml`.
13. Out of scope: editing settings from the Dashboard (item 2); quota caching, startup refresh, backoff, display states and native child restarts (item 3); Claude quota retrieval (item 4); cross-cutting lifecycle tests (item 5); a logs viewer; the task store (`config.tasks`), which is not settings.

## Settings and owners

| Setting | Owner | Type | Default | Consent |
| --- | --- | --- | --- | --- |
| `desktop_notifications` | Dashboard | bool | false | yes (OS permission) |
| `ready_sound` | Dashboard | bool | false | no |
| `automatic_local_terminals` | Server | enum on / off / default_branch_only | default_branch_only | no |
| `title_model` | Server | string provider/model, unset = titles off | unset | yes (paid calls) |
| `branch_prefix` | Dashboard | string | "feature/" | no |
| `picker_roots` | Dashboard | list of paths, `~` expanded | ~/Code, ~/src, ~ that exist | no |
| `agents` | Dashboard | list of {name, argv} | empty | no |
| `launch_choices` | Dashboard | table of project to {kind, preset} | empty | no |
| `quota.enabled` | Server | bool | false | yes (native CLI side effects) |
| `quota.codex.command` / `home` | Server | path / optional path | "codex" / unset | no |
| `quota.grok.command` / `home` | Server | path / optional path | "grok" / unset | no |

## Delivery: three PRs in order

### PR 1: loader, findings, CLI, `[quota]` move, docs, ADR

- `ovrcr-runtime` gains one settings module: typed `Settings`, per-setting effective value and source, a `Finding` list, and the off-state explanations. It replaces the three Server loaders and is what `title.rs`, `preferences.rs`, `quota.rs` and `startup.rs` read. The Dashboard's `RawSettings` parser is retired in PR 2, not here.
- `[quota]` moves to `dashboard.toml`. A `[quota]` table in `config.toml` yields the finding "quota settings belong in dashboard.toml" and configures nothing.
- `ovrcr settings` and `ovrcr settings --json`.
- Docs: `docs/dashboard.md` (the settings block gains `[quota]`, and the sentence that a parse error resets every setting goes), `docs/provider-quota.md` (enable section points at `dashboard.toml`), `docs/gui-helper.md` and `src/gui.rs` (decision 12), README table and env table, `docs/cli-reference.md` for the new command.
- Commit `CONTEXT.md` terms, `docs/adr/0007-...`, and this spec.
- Acceptance: a fixture document with one wrong type keeps every other setting and yields exactly one finding naming the key and line. An unknown key and an unknown enum spelling each yield a finding. An unparseable document loads all defaults with one document-level finding. `[quota]` in `config.toml` yields its finding and the quota workers do not start. `ovrcr settings --json` round-trips every row and finding. Existing Server tests on `title_model`, `automatic_local_terminals` and `[quota]` pass against the new loader.

### PR 2: snapshot on the wire, Dashboard reads only the snapshot

- New `ServerMessage` settings snapshot (decision 4), sent at hello and whenever the Server's reading changes; `PROTOCOL_VERSION` bump and wire fixture refresh (decision 11).
- The Dashboard drops `RawSettings` and `load_dashboard_settings`, takes every setting from the snapshot, and writes through the existing writer to the published document path (decision 10).
- Settings popup from menu and palette, footer line on findings (decision 9).
- Acceptance: the snapshot at hello equals the Server's effective settings for a document the client process cannot read. A Dashboard toggle writes to the published path and the next snapshot confirms it. The popup and footer line carry CUA evidence per `AGENTS.md` and `docs/testing-computer-use.md`. Docs: `docs/dashboard.md` popup and footer text.

### PR 3: watcher and live settings

- The 2-second stat on an existing Server tick, reload on change, republish on a real change (decision 3).
- `title_model` live on the title worker's next tick; `quota.enabled` live as decision 6 describes.
- Acceptance: editing the document while a Dashboard is attached republishes within 3 seconds. Setting `title_model` with the Server running starts titling without restart; clearing it stops. Flipping `quota.enabled` on starts collection at the next cycle without restart; flipping it off stops the worker at its next cycle. An unchanged file produces no republish. Docs: one sentence in `docs/dashboard.md` that every setting takes effect without a restart.

## Conventions for the workers

Follow `AGENTS.md`: read the affected source and callers first, isolated worktree, meaningful unit and integration tests on the real application path, never weaken an assertion, update docs and CLI help in the same PR, PR text states problem, behaviour change, verification and gaps. Merging needs Kyle's authorization. PR 2 and PR 3 are stacked: PR 2 branches from PR 1's branch and PR 3 from PR 2's; rebase when the base merges. Report significant unrelated debt instead of fixing it.

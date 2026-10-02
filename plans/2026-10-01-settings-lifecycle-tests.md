# Fresh-install and settings lifecycle tests (item 5 of the usage-defaults plan)

Decided with Kyle on 2026-10-01 over two grilling rounds. Cross-cutting acceptance for items 1 to 4; feature-specific tests stay in their owning items' PRs. Item 5 adds no product code. A failing case is filed against its owning item.

## Problem

No test exercises a clean install: the live fixture (`tests/support/live.rs`) inherits the developer's HOME and always sets `OVRCR_CONFIG`, so the default config path and an empty home are never run. Settings parsing is tested per parser, not per setting. Demo configuration (`src/gui.rs`) enables what production leaves off. Nothing proves the Server and the Dashboard read one document the same way, or that a setting survives a restart.

## Tier 1: the document matrix

Lives beside item 1's loader in `ovrcr-runtime` and is generated from the declared settings table, so a new setting gets its cases without new test code.

Per setting: missing; explicit default; explicit non-default; wrong type; near-miss misspellings (last character dropped; two adjacent characters swapped), each yielding exactly one finding that names the key and the line and leaves every other effective value unchanged.

Document-level: missing file; empty file; malformed TOML (all defaults, one document-level finding); `[quota]` in `config.toml` (its finding, nothing configured); unknown enum spelling for each enum setting; one bad value beside every other setting valid; comments and unknown tables preserved through a Server write (item 2); a symlinked document followed, never replaced; a read-only document refused on write with the document unchanged.

## Tier 2: the real-process suite

`tests/settings_lifecycle.rs` on `Live::binary`, each case with a private HOME and no `OVRCR_CONFIG`, so the default config path is the one under test. Fixture binaries stand in for providers.

1. Fresh install: no files anywhere. The Server starts. `ovrcr settings --json` shows every default with source default and no findings. The hello snapshot equals the CLI reading byte for byte.
2. Restart persistence: `ovrcr settings set`, Server restart; the value and its source survive and nothing else changed.
3. Explicit on and off: `quota.enabled` flipped in the file while a Dashboard is attached starts and stops the fixture collector without restart, and the Dashboard row follows.
4. Parity: a document the client process cannot read still yields the Server's values in the Dashboard.
5. Service versus client paths: a service definition installed with one `OVRCR_CONFIG` while the client environment carries a different `OVRCR_DASHBOARD_CONFIG`; the Dashboard shows the Server's document path and writes there, never to the client's. Isolation as in `tests/service.rs`.
6. Account change: the Codex A to B to A case is reused from `tests/provider_quota.rs`; a `claude auth status` fixture flipping `loggedIn` true, false, true moves the Claude row between its states without restart.
7. Production defaults versus demo: the loader's default table equals the TOML block in `docs/dashboard.md` (parsed, not pattern-matched); the GUI demo's `dashboard.toml` differs from production only by the allowlist held in one constant in `src/gui.rs` (`automatic_local_terminals = "on"` and the quota fragment).

## Fixtures item 5 owns

An isolated-HOME `Live` helper; a `claude auth status` fixture binary; the document generator for the matrix.

## Where it runs

In the default macOS checks job under nextest, and in the Linux jobs when dispatched. Whole suite under 60 seconds. Nothing is skipped silently: an unavailable service manager fails its case with the reason.

## Dependencies and delivery

PR 1 (the matrix and the fixtures) after item 1 PR 1. PR 2 (the seven cases) after item 2 PR A (`set` and `reset`) and item 3 PR 1 (states).

Acceptance: every matrix case passes against item 1's loader. Each of the seven cases fails when its guarded behaviour is reverted, demonstrated in the PR for at least the fresh-install, parity and defaults-versus-docs cases. Suite wall time under 60 seconds on the macOS runner.

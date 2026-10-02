# Settings editor in the Dashboard (item 2 of the usage-defaults plan)

Decided with Kyle on 2026-10-01 over three grilling rounds. Builds on item 1 (`plans/2026-10-01-shared-settings.md`): the one settings document, the loader, the settings snapshot, and `ovrcr settings`. Vocabulary in `CONTEXT.md`; `docs/adr/0007` records the Server as the only reader and the only writer.

## Problem

Settings change only by hand-editing TOML. Three keys have ad-hoc Browse toggles (`N`, `S`, `L`). The TOML writer lives in the TUI. Nothing shows which values are defaults and which are set.

## Decisions

1. Item 1's read-only Settings popup becomes the editor. It opens from the menu and the palette; no key binding.
2. Each row shows label, effective value, a source badge (default or set), the default beside an overridden value, and the row's finding if any. Reset is offered only when the source is the document.
3. Enter edits the selected row in place with the palette's existing field kinds: Toggle for booleans, Pick for enums and for a launch choice's kind and preset, Text for strings, Path for paths. Per-row immediate commit; Escape cancels only that edit; Reset removes the key so the source returns to default. A value the Server rejects shows the error on the row and leaves the document unchanged.
4. `picker_roots`, `[[agents]]` and `launch_choices` expand into child rows, one per element or entry, plus an Add row; Remove is a child-row action; no reordering. An agent's `argv` is edited as a TOML array literal (for example `["claude", "--verbose"]`) that the Server parses as an array of strings.
5. Groups with headers in one scrolling list: Alerts (`desktop_notifications`, `ready_sound`), Workspaces (`automatic_local_terminals`, `branch_prefix`, `picker_roots`), Titles (`title_model`), Usage (`quota.*`), Agents (`[[agents]]`), Remembered launches (`launch_choices`, one child row per project). The palette's word filter on `/` filters rows across groups.
6. Unknown-key findings are listed at the top with key and line; no remove or rename action.
7. While the document is unparseable, editing is disabled: rows show defaults, the top finding names the line and says editing is off until `dashboard.toml` is fixed by hand; `set` and `reset` return the same message.
8. No confirmation step when a consent setting is turned on. The row description carries the one-sentence side effect (native CLI may refresh its own auth and write logs; OS permission prompt; paid calls).
9. Write path: a new `Request::SetSetting { path, value: Option<String> }`. `path` is a setting path (`quota.codex.command`, `picker_roots[2]`, `agents[1].argv`, `launch_choices.myproj.kind`); `value` is TOML value text, `None` removes the key or element; Add is set at index len. The Server type-checks against the declared type, rejects a path that names no declared setting, applies the edit with the `toml_edit` writer moved from the TUI into `ovrcr-runtime` (comments, unrelated keys and formatting preserved; atomic replace), re-reads and republishes the snapshot. `N`, `S`, `L` and launch-choice remembering route through the same request. The Dashboard touches no file. The Server applies each write to the latest document under its own lock; external editors stay unlocked as today.
10. CLI: `ovrcr settings set PATH VALUE` and `ovrcr settings reset PATH`. Through the Server when one is reachable, otherwise a local write with the same writer, printing the document path. Value syntax: TOML value text first; a bare word is accepted verbatim only for string and path settings; mismatches are rejected naming the expected type.
11. Every setting is live, so there is no restart-required state to show.
12. Out of scope: quota behaviour and states (item 3), Claude retrieval (item 4), lifecycle tests (item 5), a logs viewer, reordering collections, renaming or removing unknown keys, replacing an unparseable file.

## Dependencies

The Server PR needs item 1 PR 1 (loader types). The Dashboard PR needs item 1 PR 2 (snapshot). A new `Request` variant bumps `PROTOCOL_VERSION`. An `[[agents]]` row whose name matches a detected agent still overrides its argv as today. A set or reset of an alert setting applies when the snapshot arrives.

## Delivery: two PRs

### PR A: Server side

`SetSetting` request and response; writer and validator moved into `ovrcr-runtime`; `N`, `S`, `L` and launch-choice remembering rerouted through the request; `ovrcr settings set` and `reset`; protocol bump; `docs/cli-reference.md`, `docs/dashboard.md` and the env table (the Server resolves `OVRCR_DASHBOARD_CONFIG`).

Acceptance: set and reset on every setting type round-trip through the request and the file keeps comments, unrelated keys and formatting (the existing persistence tests extended to the Server writer). An invalid value is rejected naming the expected type and the document is unchanged. `N`, `S`, `L` produce the same document as before, through the request. Remembering a launch choice preserves other projects' entries. An unknown path is rejected. With no Server running, `ovrcr settings set` writes the resolved document and prints its path, and a Server started afterwards loads that value.

### PR B: Dashboard side

Editable rows, child rows for collections, groups, `/` filter, findings behaviour, disabled editing on an unparseable document, consent sentences in row descriptions. CUA evidence for editing a toggle, an enum, a string, a collection element, a reset, and a rejected value. `docs/dashboard.md` updated.

## Conventions for the workers

As in item 1's spec: `AGENTS.md`, isolated worktree, real tests on the application path, never weaken an assertion, docs in the same PR, PR text with problem, behaviour change, verification and gaps, merge only with Kyle's authorization.

# Task 6 dashboard presentation

Base: `8586a2574dc4de5562fd1451bcf63154848644a8`, plus uncommitted dashboard diff. Coordinator owns commit and diary.

The existing label row shows bound activity quality (`idle observed` or `idle confirmed`) or reporter `unavailable`. The existing context row uses the bound metrics even when unknown, retaining the established stale `~` suffix and adding `uncertain`/`estimate` where applicable. Legacy unbound display remains unchanged.

The existing second metadata row shows `tokens conv partial 25  cost inv estimate $0.00` (conv = conversation, inv = invocation). Token totals sum input and output once; cache/reasoning subsets are not added again. Missing cost is `—`. The cost amount rounds integer ticks to cents with u128 intermediate arithmetic. Usage and cost independently mark `stale` using their component receipt age and the existing five-minute cutoff, or `uncertain` for source uncertainty. Single-pane History retains its metadata hint. No row dimensions or input/state authorities changed.

## Evidence

- attempt01: compiler failure, zero tests; context types initially imported from wrong module.
- attempt02: behavioral RED using original renderer plus the new regression: 1 failed, missing `idle observed`.
- attempt03: owning TUI library: 38 passed, zero failed/ignored.
- attempt04: root `tui`: 163 passed, zero failed/ignored; includes existing literal geometry, Unicode, legacy context and selection assertions.
- attempt05: owning-package clippy with warnings denied: exit 0.
- attempt06: changed-file rustfmt check and diff check: exit 0.

The new actual-draw regression covers quality, independent stale usage with current context/cost, partial tokens (including avoiding cache double count), unknown context after clear, absent bound metrics with legacy data retained, absent/zero/estimated cost, uncertainty, unavailable health, no binding identifier disclosure, tiny widths, and both split metadata rows. These are headless rendering checks, not native macOS/Linux GUI acceptance. Narrow panes clip metadata through existing Ratatui bounds; full details require sufficient width or CLI inspection.


## Native review correction: selected header activity

Coordinator native screenshot/accessibility evidence `task7-gui/04-synthetic-metrics.*` showed sidebar `idle observed` but selected header plain `agent idle`. The selected header still used the legacy activity field. It now uses the same managed activity/quality/health helper as the sidebar whenever an agent snapshot exists, retaining the legacy rendering for unbound sessions. Split layout was not redesigned.

Attempt07 is actual-draw RED with legacy Busy deliberately conflicting with managed Idle/Observed. The regression specifically requires header strings `agent idle observed`, `agent idle confirmed`, and `agent unavailable`. Attempt08: all38 owning-library tests passed. Attempt09: all17 root sidebar tests passed, including existing unbound legacy metadata/lifecycle assertions. Attempt10 owning clippy passed; attempt11 diff check passed. Coordinator owns final GUI rebuild/recapture; these checks alone are not native acceptance.

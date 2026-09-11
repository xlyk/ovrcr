# Task 1 final rendering correction

Assigned product baseline: 2db1897. Coordinator advanced HEAD during work to b661988361958b07131ea9e2e0bd76a0ec780666 for unrelated work. Worker edited only `crates/ovrcr-tui/src/dashboard/render.rs` and `crates/ovrcr-tui/src/dashboard/tests.rs`; all other modifications belong to coordinator/other workers and were preserved. No staging or commits.

## Change

Unavailable health now precedes the retained Ready text. Sidebar rows for unavailable Ready place that status before the user label, so a long background Codex label cannot hide the health warning. Single-pane Ready metadata places process status first, then health and Ready, with elapsed time appended only if it fits. Clipping occurs after those statuses. Split metadata reuses the same health-first activity string, keeping its existing process-first priority and fitting behavior. Non-Ready sidebar and metadata behavior is unchanged.

## Regression evidence

`task-1-final-red.log`: `rtk proxy env CARGO_INCREMENTAL=0 cargo test -p ovrcr-tui --lib provider_dashboard_preserves_quality_unknowns_and_component_age`; exit 101; 1 failed, 0 passed, 49 filtered. Behavioral failure against original rendering: actual background Codex sidebar row was `     ├ long Codex label that cannot fi…`, hiding both unavailable and Ready. No compiler RED claimed.

`task-1-final-green.log`: same command; exit 0; 1 passed, 0 failed, 49 filtered. Actual sidebar-row assertions cover a background Codex session (a different session is focused) with an overlong label at widths 80, 100, 120. Actual single-pane metadata-row assertions cover those three widths in both running and exited states, requiring process status plus unavailable and response ready. The existing 40 split-pane row combinations remain and pass, including health, process, quality, and clipping checks. Old assertions expecting health to be hidden by clipping were replaced with the above explicit intended behavior.

`task-1-final-fmt.log`: `rtk proxy cargo fmt --all`, exit 0.
`task-1-final-format-check.log`: `rtk proxy cargo fmt --all -- --check`, exit 0.
`task-1-final-diff-check.log`: `rtk proxy git diff --check`, exit 0.

All failures retained; no broader suites or new native acceptance claimed. Self-reviewed status ordering and shared split caller; no provider/runtime changes. Ready for coordinator commit and focused rereview.

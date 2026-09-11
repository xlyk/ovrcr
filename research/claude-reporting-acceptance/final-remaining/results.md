# Final remaining verification results

The final macOS workspace run at `366c5d3` used all targets and all features.
`macos-tests-attempt2.log` records 599 passed, 0 failed, and 11 intentionally
ignored tests across 24 test binaries. The matching full-workspace clippy and
formatting checks passed.

The first macOS attempt is preserved in `macos-tests-attempt1.log` with exit 101.
It failed to compile because five runtime test fixtures omitted the
acceptance-diagnostics `dashboard_monitor` field. The fixture-only correction was
made and the complete second run above passed at `366c5d3`; the initial compiler
failure is not counted as a product test failure or as a passing run.

The workspace doctest command exited 0, but `doc-tests.log` records zero tests in
each crate. It verifies that doctest discovery and compilation did not fail; it
does not provide behavioral test coverage.

Later changes were confined to the metrics-row UI and its evidence. The affected
OVRCR TUI suite passed all 38 tests at `ba7d560` and again after the final compact
fallback at `7ec5634`. Full all-features clippy and formatting checks also passed
at `7ec5634`. Per the plan, the full 599-test workspace run was not repeated for
these later UI-only changes. The corrected native smoke is recorded in
[`native-corrected/results.md`](../native-corrected/results.md).

The earlier actual Linux baseline passed 562 tests with 0 failures and 11
intentional ignores. The revised Linux run and the Linux 50-session capacity load
remain blocked because Docker became unavailable. No hosted CI execution is
claimed.

These results verify the stated macOS suite, affected TUI checks, corrected native
layout, and earlier Linux baseline. They do not close the broader milestone:
provider settled completion and complete accounting remain unsupported, and the
revised Linux/load, kernel-backlog, and full-heap gates remain open.

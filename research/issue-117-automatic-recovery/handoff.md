# Issue #117 development handoff

Native GUI/provider acceptance belongs to [#127](https://github.com/xlyk/ovrcr/issues/127).
This handoff is not native acceptance or merge approval. The implementation is on
`implement-task-117`, based on `1af3bb47b9a6e2cf335978a2b06b70ffdfb0acd5`.
Pin the implementation commit supplied with this handoff before testing.

## Behavior

A visible nonzero Dashboard pane requests one `RecoverSession` for an eligible
interrupted Agent. The server rechecks eligibility and uses the existing locked
reopen/spawn path. Live runs are reused. Inventory, hidden assignments, shells,
stopped/exited rows, unarchived rows, invalidated identities and unsupported
adapters do not launch automatically. Same-boot and unverifiable ownership still
require explicit acknowledgement that old agents and descendants stopped.

Prerequisite/capacity failures persist a diagnostic and require explicit Retry.
Natural Agent exits persist an explicit-action diagnostic without certifying
process ownership. Old launch receipts do not select panes or install old runs.
The wire protocol is **18**; restart only task-owned older servers for this test.
The SQLite schema and provider registration are unchanged.

## Reproduce automated coverage

Run from the pinned checkout. Fixtures own temporary configs, sockets, Git
workspaces, provider files and process groups; no live user configuration is used.

```sh
cargo test --all-features --test retained_sessions -- --test-threads=1
cargo test -p ovrcr-tui --lib
cargo test -p ovrcr-protocol --lib
cargo check --workspace --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets --all-features --no-fail-fast
node --test tests/pi_reporting_extension.mjs
```

`tests/retained_sessions.rs::prepare_interrupted_claude` creates an authoritative
managed conversation with the existing controlled provider helper. It stops its
owned processes before simulating an interrupted run from a different boot.
Only the provider is controlled: launch, Reporter, PTY, socket, Dashboard view
requests and server admission are production code. The ignored helper test is
executed as the fixture's native child, not omitted coverage.

| Coverage | Evidence |
| --- | --- |
| Display/request/view handshake, exact UUID, no prompt/Enter | Real Dashboard connected to a real binary server; replacement Screen and final acknowledgement; captured native argv |
| Duplicate launches | Automatic/explicit/automatic requests queued before reading receipts; all name the same successor; exactly one native resume |
| Reconnect and close | Reconnected live row emits no recovery; delayed pre-close request cannot restart the row |
| Ownership and disposition | Server rejects same, absent and malformed boot IDs, stopped, archived, returned and invalidated records |
| Failure and capacity | Missing history stays failed after restoration/restart; 50 live shells block recovery; freeing a slot still requires explicit Retry |
| Natural exit | Attached Agent exits, simulated reboot occurs, automatic request remains rejected |
| TUI negatives and receipts | Hidden inventory, zero geometry, shell/stopped/exited/live/archive/ack/unavailable/failure cases; stale receipt preserves focus and current error; explicit Retry label/action |

## Verification recorded on macOS

Host: Darwin 25.5.0 arm64; Rust 1.98.0; Node 22.23.2.
The controlled Claude executable reports 2.1.268. Native admission support remains
exact Claude Code **2.1.267 / 2.1.268**; no other adapter is added.

- Retained-session suite: 11 passed, one helper ignored by the outer runner.
- TUI unit suite: 162 passed. Protocol suite: 37 passed.
- Check, Clippy with warnings denied, formatting and Node tests (17) passed.
- Full no-fail-fast run: **965 passed, 2 failed, 19 ignored**. Failures were the
  existing PTY-backpressure test and one controlled-Claude attachment timeout.
  The retained suite subsequently passed in parallel and serial runs.
- The PTY-backpressure failure also reproduces in an independently built archive
  of base `1af3bb4`, at `tests/server_lifecycle.rs:1773`. Its next-frame assertion
  does not distinguish a response from an asynchronous event. The exact incoming
  frame remains uninvestigated; this is not declared fixed or waived.
- A final serial full-suite attempt passed retained-session coverage but hit the
  same backpressure failure and then exceeded the 360-second command deadline
  during runtime tests. It is **not a passing full-suite result**.
- Logs, including failed attempts: `/tmp/ovrcr-117-evidence/`. Independent review,
  hosted CI, Linux execution and #127 native acceptance remain outstanding.

## Native acceptance launch and fixture boundaries

Use [the computer-use guide](../../docs/testing-computer-use.md) from the pinned
checkout: `rtk proxy just gui` creates a disposable application/server/workspace.
Keep its launcher output and owned PID/PGID inventory. Do not attach it to a user's
live server. Use the guide's retained-session/server-loss procedure and the
[Claude recovery contract](../../docs/development/recovery.md).

Use an installed supported Claude executable, authorized native credentials and
configured reporting hooks in the acceptance fixture. Preserve the recorded
`CLAUDE_CONFIG_DIR`, history, executable and cwd references. Capture one certified
conversation before interrupting it. Reboot simulation may change **only the
stopped fixture's database**, after independently verifying its old processes
and descendants are gone; the automated fixture shows the exact metadata setup.
Do not claim that simulation is an actual reboot.

Capture screenshots and accessibility evidence for first visible recovery without
Enter, a dormant undisplayed row, duplicate display/reconnect, same-boot refusal,
unavailable prerequisites, Retry, and close/unarchive. Confirm the old conversation
visibly continues and distinguish launch from native attachment. Run native
provider work only within the separately authorized acceptance scope. Clean up
only the recorded task-owned resources, preserving evidence and uncertain state.

## Local review

Standards and spec were reviewed separately against the base diff. No remaining
code blocker was identified. The natural-exit persistence gap and missing explicit
Retry label found during development have regression tests. The harness has no
subagent tool, so this is a local self-review, not independent review. Required
verification remains blocked as listed above; neither #112 nor #127 is closed.

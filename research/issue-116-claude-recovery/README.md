# Issue #116 verification

Implementation base: `cbbcf19dd93ec861e3d22361afff759dc947e3b5` (merged #114).
Branch: `codex/issue-116`. This record accompanies the implementation commit.
Issue: https://github.com/xlyk/ovrcr/issues/116

## Initial implementation evidence (macOS, fb01a83)

- Final `SHELL=/bin/sh cargo test --workspace --all-targets --all-features --locked --offline`: **943 passed, 0 failed, 19 ignored**, across 31 test binaries. Ignored tests are not claimed as acceptance. The retained-session helper is invoked explicitly by the real-PTY fixture.
- `cargo check --workspace --all-targets --all-features --locked --offline`: passed.
- `cargo clippy --workspace --all-targets --all-features --locked --offline -- -D warnings`: passed.
- `cargo fmt --all -- --check` and `git diff --check`: passed.
- Controlled Claude executable through the production managed launcher, real PTY and Reporter verifies fresh and explicit-resume admission; exact UUID/argv with no prompt; two server restarts before callback; same row/title/reference; missing history/executable/configuration/directory without launch; and invalidation before resumed attachment.
- SQLite trigger failures exercise retry after initial retention failure and unsupported-transition invalidation failure. Dashboard tests cover original-provider selection for the separate Start new conversation action.
- Standards and requirements reviewers re-reviewed corrections and reported no remaining blocking findings in their bounded source reviews.

The first full run failed two lifecycle cases: `agent_admission_branch_freezes_without_replacement_or_reopening` (fresh UUID assertion) and `agent_admission_failed_bind_status_disconnects_watch` (callback observation deadline). Lost-bind settlement/watch cleanup was corrected; the focused watch test and final full suite passed. Original failures are retained in `/private/tmp/issue116-workspace-tests.log`; final output is `/private/tmp/issue116-workspace-tests-final.log`; lint output is `/private/tmp/issue116-clippy-final.log` on the implementation host.

## Open acceptance gates

- Native paid Claude continuity was not run: the bounded-account authorization request remained unanswered. Controlled fixtures establish the launch contract, not upstream conversation continuity.
- The actual disposable macOS GUI rendered and exposed accessibility state, but key/click actions did not establish a changed visible outcome; clipboard paste timed out. One Return action was rejected by automatic approval review because truncated accessibility text looked like unverified shell input. No successful native GUI interaction is claimed.
- The disposable GUI was closed. Known fixture shell PIDs 41042 and 63491 and the issue-116 GUI executable were absent in the final process check; the original fixture directory was removed. A replacement fixture root was not recorded independently.
- Linux tests and Linux native GUI acceptance were not run locally.
- No merge or release; the broad recovery roadmap remains unchecked.

## Durability boundary

The server acknowledges retention/invalidation only after successful SQLite persistence. Failed requests remain retryable. An observed invalidation blocks resume in memory even when persistence fails. Persistent storage failure followed by owner death before a successful retry can leave the last committed reference on disk; this implementation cannot certify that uncommitted invalidation as durable.

## Shared-interface amendment and archive integration

The user authorized preparing a common interface for concurrent #117–#119 development. Commit `4328123` adds the provider-tagged reference and common adapter/Reporter boundary. Commit `16dd21b` integrates archive prerequisite #115 from main `c633103`; its source was independently reviewed. See [the handoff contract](../../docs/development/recovery.md).

- Schema 5 recognizes archive-main schema 3, earlier Claude-draft schema 3 and generic schema 4 by their structural markers. Migration preserves disposition, exact references and invalidation; offline reads do not migrate. Protocol 17 combines the archive and recovery wire changes.
- Retained-session integration: 6 passed, 1 intentionally ignored executable helper. Schema migration tests: 2 passed. Protocol snapshot: passed. Reporter module after the binding correction: 17 passed.
- Typecheck, Clippy with warnings denied, formatting and diff checks passed on the integrated code.
- The retention retry regression first failed when a fresh binding inherited a pending old reference. A second regression reproduced background Poll/NativeCompleted promoting old metadata after A→B→A or forced same-identity rebind. Clearing the pending retention on accepted binding changes resolved both; both review axes cleared the correction.
- Pre-integration parallel full runs failed separately in `timeout_cancel_and_parent_loss_remove_detached_descendants` (condition deadline) and `claude_metrics_missing_initial_file_preserves_activity_and_recovers_same_path` (native fixture fell back with admission unavailable and missed its callback marker). The task-runner test passed alone. No unrelated code or test assertions were changed. Logs remain `/private/tmp/issue116-neutral-workspace.log` and `/private/tmp/issue116-neutral-workspace-final.log`.
- A newly added legacy test initially selected the first inventory row rather than its exact session ID; corrected. An intermediate wire snapshot replacement had a declaration syntax error; corrected before checks. Failed attempts remain under `/private/tmp/issue116-neutral-retained*.log` and `/private/tmp/issue116-neutral-wire.log`.

Integrated full suite at `16dd21b`: **954 passed, 0 failed, 19 ignored** across 31 binaries. Command: `SHELL=/bin/sh RUST_BACKTRACE=1 cargo test --workspace --all-targets --all-features --locked --offline -- --test-threads=1`. Test cases were serialized; each test's internal concurrency remained enabled. Log: `/private/tmp/issue116-integrated-workspace.log`. Clippy log: `/private/tmp/issue116-integrated-clippy.log`. Native provider, GUI and Linux acceptance gaps above remain open; no paid provider run, merge into main or release was performed.

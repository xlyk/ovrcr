# Issue #116 verification

Implementation base: `cbbcf19dd93ec861e3d22361afff759dc947e3b5` (merged #114).
Branch: `codex/issue-116`. This record accompanies the implementation commit.
Issue: https://github.com/xlyk/ovrcr/issues/116

## Automated evidence (macOS)

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

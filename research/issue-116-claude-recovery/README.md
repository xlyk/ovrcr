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


## 2026-09-17 native GUI follow-up

The earlier GUI-input and hosted-Linux gaps above are superseded by these observations. Native Claude continuity remains unrun pending account-use authorization. Interactive Claude does not enforce `--max-budget-usd` (the option is print-only); the pending request instead bounds the test to two short prompts and two resume launches. The official 2.1.268 executable and isolated configuration were prepared without invoking the account. The user's default 2.1.274 installation was not changed.

- All four hosted checks passed at `8c6d9a4`: macOS checks, Linux, Linux capacity, and Linux memory high-water. [Run 35191550562](https://github.com/xlyk/ovrcr/actions/runs/35191550562). This is automated Linux evidence, not Linux GUI evidence.
- The actual rebuilt macOS GUI accepted keyboard input and rendered the distinct command output `FINAL_INPUT_116_OK`. [Screenshot](native-20260917/05-final-input.jpg), [accessibility text](native-20260917/05-final-input.ax.txt).
- A disposable managed Claude-kind row used only `/usr/bin/true`; no paid provider ran. Its missing certified identity exposed two real UI defects: the unavailable action gave a generic unsupported-provider reason, and the exited header hid the recovery diagnostic. The fixes show the retained server reason and preserve actionable recovery text after exit. [Fixed header](native-20260917/06-fixed-header.jpg), [fixed dialog](native-20260917/07-fixed-unavailable-dialog.jpg), with matching `.ax.txt` evidence. Confirming the dialog left the same row exited with run 1; [inventory](native-20260917/final-inventory.json).
- The earlier fixture also exposed the separate Start new conversation action and opened the form with Claude selected; it was canceled before submission. [Form screenshot](native-20260917/03-new-conversation-form.jpg). This proves the GUI action/form, not a real Claude launch.
- Three regressions were observed failing before correction: generic unavailable reason, hidden exited diagnostic, and stale Busy activity with empty recovery metadata. The final status logic distinguishes actual diagnostics from empty records while preserving existing Ready behavior. An intermediate attempt to suppress all exited activity failed two existing tests and was corrected rather than changing those assertions.
- Final focused checks: `cargo test -p ovrcr-tui` **158 passed**; `cargo test --test tui` **203 passed**; workspace/all-targets/all-features Clippy with warnings denied, formatting and diff checks passed. Logs: `/private/tmp/issue116-status-final2.log`, `/private/tmp/issue116-tui-final3.log`, `/private/tmp/issue116-gui-clippy.log`. Earlier failures remain in `/private/tmp/issue116-unavailable-red.log`, `/private/tmp/issue116-header-red.log`, `/private/tmp/issue116-empty-recovery-red.log`, and `/private/tmp/issue116-status-final.log`.
- Both independent review axes cleared the final correction in a bounded source re-review; they did not independently rerun tests.
- All three disposable GUI launches were closed. Recorded GUI/server/dashboard/shell PIDs were absent afterward; final owned session process groups and fixture root were also absent. See `native-20260917/cleanup-*.json` and corresponding process inventories. No live user server was stopped.

Still required: isolated native Claude continuity and repeat restart before another prompt. No account-backed test, merge, release, or issue closure is claimed.


## 2026-09-17 authorized native continuity completion

The previously open native provider gate is now **passed** on source `8347563`: exact same conversation and row survived two controlled server restarts and two explicit native GUI resumes before a second prompt. Claude correctly recalled the first prompt's marker. Only the approved two prompts were sent. See [native evidence and boundaries](native-continuity-20260917/README.md). This supersedes earlier authorization/native-continuity blockers; earlier failed attempts remain recorded. All four hosted checks passed on this source in [run 35235777003](https://github.com/xlyk/ovrcr/actions/runs/35235777003). No native Linux GUI claim, mainline merge, or release is made.

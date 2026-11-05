# Integrated verification — 2026-11-05

Reviewed source: `78ce52fd62ddc4522e53fe23afd97d29e6fe60c4`, integration worktree. Later evidence-only commits do not change the tested source. [Attempt index](attempts.json) records actual exits and executed counts, including the failed original full run and draft compilation failures.

## Environment

- Stable dedicated target: `/Users/xlyk/Code/ovrcr/.worktrees/ux-pass-integration/target`; no unrelated Cargo process uses it.
- `xcargo` holds the dedicated lock across source-mtime refresh, build and execution. The previous shared target was left alone. Earlier shared-target results showing obsolete wording/executable replacement are not acceptance evidence.
- Every authoritative runner has a disposable HOME, clears inherited OVRCR/provider/XDG authority and retains ordinary Cargo/Rustup cache locations. Local sockets, PTYs and process checks use granted execution permission. No personal provider credentials or paid model runs.
- Final workspace command: `CI=1 RUST_TEST_THREADS=1 XCARGO_TIMEOUT=5400 cargo test --workspace --all-targets --all-features --no-fail-fast`, invoked through the private-HOME runner and xcargo. Existing test deadlines/assertions remain unchanged; 5400 seconds is the outer whole-job watchdog.

## Results before the final whole-workspace rerun

| Gate | Result |
| --- | --- |
| First whole workspace at 76e3228b | 2,591 passed, 8 failed, 32 ignored; 53 completed binaries; exit 101 |
| Corrected terminal acceptance | 23 passed, 1 ignored; exit 0 |
| Corrected GUI acceptance, serial | 10 passed, 0 ignored; exit 0 |
| Clippy all features / default features | Both exit 0 with warnings denied |
| Formatting / whitespace | Passed |
| Narrow project attention regression | Failed before correction, passed after |
| ED3 saved-cursor regression | `l10-review-red-preserve-state-4.log`: actual assertion failure; `l10-review-runtime-preserve-state-2.log`: all three related tests pass. Unsuffixed draft logs failed compilation and prove no behavior. |

The original workspace failure is not relabeled green by corrected target runs. The fresh whole-workspace job completed: **2,599 passed, 0 failed, 32 ignored**, across 53 test binaries; exit 0. See [final result](final-result.json) and [full log](integrated-workspace-final.log).

Ignored subprocess entrypoints may be exercised by their owning parent tests; their ignored status itself is not a pass. Installed-provider, dedicated capacity/memory, and other explicitly gated checks remain unrun unless a separate receipt says otherwise. Linux, hosted CI and live-provider behavior are outside this local verification. Native CUA evidence and cleanup are separate in [the native index](../after-2026-11-05/README.md).

Readable AX/log text has line-end padding and excess final blank lines removed for repository whitespace checks. Exact original bytes are preserved in [the raw evidence archive](raw-evidence.tar.gz), with SHA-256 entries in [the manifest](raw-evidence-manifest.json). No failed attempt or test result was replaced.

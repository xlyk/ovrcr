# Issue #138: shared Dashboard settings publication

Issue: https://github.com/xlyk/ovrcr/issues/138

Tested checkout: `chartreuse-acai`, branch `implement-task-138`, based on
`2dd2bd1b0065855d9634f98c1eefb33cf620ba72`. Tests ran against the implementation
working tree; the final commit includes those source files unchanged.

## Behavior

- Alert edits and remembered launch choices share read, typed validation,
  comment-preserving edits, and synchronized temporary-file/atomic publication.
- Valid symlinks and permissions survive; invalid documents, dangling links and
  read-only targets are rejected. Saves retain prior unrelated external edits.
- Failed replacement drops its owned temporary file before returning the error.
- Alert failures retain active preferences. Launch-save failures retain the
  selected session and in-memory choice. Their warning survives both direct and
  coalesced view acknowledgements; the acknowledged session accepts input.
- Settings schema, location, loader behavior and launch semantics are unchanged.

## Regression evidence

1. `successful_launch_preserves_symlinked_commented_settings` failed against the
   original writer because it replaced the symlink. It passes through real
   Dashboard Agent and Terminal launch-success responses after consolidation.
2. `failed_replacement_cleans_temporary_file_before_returning_error` exposed a
   retained temporary file inside `PersistError`. Dropping that file before
   returning the underlying I/O error makes the regression pass.
3. The first native attempt showed the save warning disappearing. The added
   `launch_save_warning_survives_selection_ack_and_session_accepts_input` failed
   after acknowledgement, then passed after fresh errors revoked older banner
   ownership. It covers immediate and coalesced views and actual input bytes.

The Dashboard filesystem matrix drives `N`, `S`, Agent and Terminal, using real
isolated files and fresh reloads. It covers malformed/incorrectly typed settings,
valid/dangling links, permissions, missing parents/private files, external edits,
exact project keys, publication failure, cleanup and rendered notices. Existing
launch failure, Nothing yet, restart and one-off-command assertions remain.
Module tests additionally cover ordinary, inline and dotted TOML table styles.
The non-writable-directory case requires an unprivileged test user.

## Automated checks

Final checks ran on macOS. Node version: `v22.23.2`.

| Command | Result |
| --- | --- |
| `cargo test -p ovrcr-tui --lib` | 173 passed |
| `cargo test -p ovrcr --test tui` | 204 passed |
| `cargo check --workspace --all-targets --all-features` | Passed |
| `cargo test --workspace --all-targets --all-features --no-fail-fast -- --test-threads=1` | 991 passed, 0 failed, 20 existing ignored; 31 binaries |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | Passed |
| `cargo fmt --all -- --check` | Passed |
| `git diff --check` | Passed |
| `node --test tests/pi_reporting_extension.mjs` | 18 passed |
| `node --test tests/omp_reporting_extension.mjs` | 25 passed |

[Commands and per-binary counts](automated/commands-and-results.txt),
[full-suite output](automated/23-full.log), and separate failed attempts are
retained here. All original command logs remain under
`/tmp/ovrcr-138-evidence/`. Committed text captures trim trailing whitespace;
original captures retain it. Native source snapshots are gzip-compressed copies.

Earlier attempts, not passing gates:

- Initial full suite: 108 passed, 1 failed before stopping. A background shell
  inherited ignored SIGINT/SIGQUIT dispositions; the doctor interrupt test saw
  a normal exit instead of SIGINT. The foreground regression passed for all four
  signals. The corrected runner restores default signal dispositions.
- Corrected parallel full suite: 988 passed, 2 failed, 20 ignored. Both failures
  were in `retained_sessions`, reporting agent admission unavailable. The same
  suite passed serially: 16 passed, 2 existing helper tests ignored. No runtime
  code or assertions were changed; final gates run serially to reduce contention.

## Native macOS acceptance

First attempt: alert persistence across restart, symlink/comments, and separate
`CUA_138_USABLE` output passed. The required save-warning text was absent from
screenshots and accessibility text. This exposed regression 3 above; it is not
counted as a pass. Cleanup succeeded: launcher exit 0, all 14 recorded process
groups absent, fixture root and socket removed.

Final corrected native attempt: **passed** against the rebuilt checkout.

- [Alert enabled](native-final/01-toggle.jpg), with comments retained on disk.
- [Restarted Dashboard](native-final/02-restarted-menu.jpg) offers “Disable desktop
  notifications”, confirming the saved value loaded.
- [Read-only save warning](native-final/04-warning-before-input.jpg) is visible
  with the new session selected and its shell prompt ready.
- [Usable terminal](native-final/05-usable.jpg) prints a separate
  `CUA_138_USABLE` line, not just command echo.
- Matching full accessibility text accompanies every screenshot. Settings remain
  unchanged after the failed save. [Cleanup](native-final/cleanup.json) confirms
  all 14 recorded groups absent, root/socket removed, and launcher exit 0.

See the [native report](native-final/review.md) for owned paths/PIDs, launcher
results and sandboxed-launch failures. The first attempt's screenshots and
[report](native-initial/review.md) remain separate; none are relabeled as passing.

## Review and remaining gaps

Independent [Standards](review-final-standards.md) and
[Spec](review-final-spec.md) reviews covered the original base and full source
diff, including the acknowledgement correction: **zero code findings on either
axis**. Their then-pending automated/native evidence is recorded above.

Linux filesystem verification is **unverified**. The Docker capability probe
hung and was interrupted by the user; it was not retried. Linux CI was not
dispatched. The 20 top-level ignored provider/capacity/helper tests are not
counted as passes; separate real-provider and capacity acceptance was not run.
No push, PR, or merge is included in this local commit.

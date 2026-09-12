# Testing and acceptance

- Test the real entry point that changed. A passing helper test is insufficient if the event loop, dispatcher, or writer bypasses that helper.
- For behavior changes, add a regression that demonstrates the defect before fixing it where practical. Distinguish compiler failures, printed reproductions, and failing behavioral assertions in the report.
- Map each acceptance requirement to an actual assertion. Include partial acknowledgements, wrong/stale IDs, A→B→A reassignment, coalesced changes, empty/tiny layouts, failure paths, and late cleanup where relevant.
- Preserve existing assertions when migrating fixtures. Send current-revision output when testing output handling; an ignored stale event cannot prove that output was processed safely.
- Prefer deterministic synchronization over sleeps for races. Force the disputed ordering or boundary; starting a thread does not prove it reached a lock.
- Use real PTYs, sockets, Git repositories, and process groups for claims about those boundaries. Query kernel PTY size with `MasterPty::get_size()` or `stty`; parser dimensions are different evidence. Verify actual child output, not command echo.
- Reuse existing test fixtures in their owning test binary. Confirm filters execute tests; zero tests is not a passing gate.
- Run focused checks while iterating, then the affected suites once when ready. Repeat only after relevant changes, failures, or a concrete unresolved concern.
- Save attempts under distinct filenames with the command, exit status, executed counts, and tested revision. Preserve failures; never overwrite them with a passing run.
- Separate headless, native macOS, Linux, clipboard, and provider evidence. A build or emitted OSC52 bytes does not prove native input, paste, or conversation restoration. Report unavailable checks as unverified.

## Commands and isolated acceptance

For Rust changes, choose the owning package and add a focused test filter as appropriate:

```sh
cargo test -p ovrcr-tui --lib
cargo test -p ovrcr --test tui
cargo check --workspace --all-targets --all-features
cargo fmt --all -- --check
git diff --check
```

At the feature acceptance checkpoint, run the assigned gates and the workspace regression/lint checks; do not repeat them after every edit:

```sh
cargo test --workspace --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

- Keep documentation-only verification proportional; do not run Rust suites for prose edits.
- Give every live fixture its own `OVRCR_CONFIG`, `OVRCR_SOCKET`, and temporary workspace. Use the required execution permission for PTY/socket/process tests; sandbox denial is not an application failure.
- Follow [Computer-use testing](../testing-computer-use.md) for `just gui`. Use the exact checkout under review and fresh screenshots/accessibility state.
- Record owned fixture PIDs/PGIDs and paths, retain the launcher result, and verify cleanup. Permission denial does not prove a process group is gone. Stop a hung owned test before starting another.
- Never bypass a tool's access denial through another control channel or modify the user's live agent configuration to make an acceptance check pass.
- The shared initial-admission lifecycle cases are timing-sensitive on a loaded host: the product's `--version` probe has a fixed one-second budget, and a saturated macOS host can stretch the fixture's `/bin/sh` reply past it, yielding `supervisor must select a fresh UUID` after the designed `agent admission unavailable` fallback. Read a recurrence as contention unless the evidence says otherwise; see [issue #62 diagnosis](../../research/issue-62-lifecycle-diagnosis/README.md) for the measured timeline and the coverage the `env_lock()` serialization gives up.

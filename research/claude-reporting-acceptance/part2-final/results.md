# Part 2 final local gates

Product revision: `958527167fc4d6f9e36502ece593d6bdd67e86c4`.
Independent review approved its correction to permanently close contradictory
initial resume admission and preserve fresh positional prompt support.

`commands.json` records actual exits. All commands used `CARGO_INCREMENTAL=0`.
The all-targets/all-features macOS workspace gate passed **639 tests, 0 failed,
11 ignored**, across 24 test result blocks. Workspace Clippy with warnings denied
and formatting checks passed. The doctest command passed while executing **zero
doctests**, across five crates. Native acceptance is recorded separately in
`../native-resume-integrated/results.md`.

Earlier failures remain in `../part2-proof/` and `../part2-final-attempt01/`.
Two earlier pinned-version fixture probes returned unsupported; their low-level
cause remains unestablished. Focused reruns and this full run passed without
changing deadlines or assertions. The first full run subsequently exhausted disk
space while saving the doctest result; that doctest exit was not retained and is
not counted as passed. Only this worktree's regenerable incremental cache was
removed, recovering approximately 2.8 GiB free space; subsequent Cargo runs disabled
incremental compilation. Current green results do not erase those failures.

Raw Cargo output is retained with only extra final blank lines removed from the
workspace-test and doctest logs for repository whitespace checks.

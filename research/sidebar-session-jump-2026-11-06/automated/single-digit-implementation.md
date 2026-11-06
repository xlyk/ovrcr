# Control + single-digit sidebar jump implementation

- Date: 2026-11-06
- Branch: `codex/sidebar-jump-single-digit`
- Commit: `3f311857c9e8a7f20f37c3b64d3e0dab9d9ca024` (`feat(tui): use Control single-digit sidebar jumps`)
- Base: merged integration tip `99eb4cee`
- Worktree: `/private/tmp/ovrcr-401-single-digit`
- Tree: clean after commit

## Requirement to regression coverage

- Ctrl+1 through Ctrl+9 select the first nine eligible visible Agent rows immediately; scrolling/search expose later rows without labels. Public Dashboard coverage includes 10 and 50 Agents, clipped rows, scrolling, held-input and release/readiness handling: 8 tests passed in [ovrcr-401-single-digit-public-dashboard-green-03.log](ovrcr-401-single-digit-public-dashboard-green-03.log.gz).
- Compiled app acceptance exercises the real terminal path for single-digit, retained-session, and Operator behavior: 3 passed in [ovrcr-401-single-digit-compiled-jump-acceptance-green-03.log](ovrcr-401-single-digit-compiled-jump-acceptance-green-03.log.gz). The compiled CLI Command-digit no-jump test passed 1/1 in [ovrcr-401-single-digit-command-nonjump-acceptance-green-01.log](ovrcr-401-single-digit-command-nonjump-acceptance-green-01.log.gz).
- GUI adapter tests cover physical Control wire events (crossterm Control codes 57442/57448): 2 passed in [ovrcr-401-single-digit-gui-input-green-06.log](ovrcr-401-single-digit-gui-input-green-06.log.gz). The corrected encoder is exercised through the compiled Control jump acceptance above.
- GUI integration regressions cover Control selection into a real Agent (1/1), Command shortcuts not activating the gesture (1/1), and focus-loss cancellation (1/1): [ovrcr-401-single-digit-gui-control-acceptance-green-02.log](ovrcr-401-single-digit-gui-control-acceptance-green-02.log.gz), [ovrcr-401-single-digit-gui-command-nonjump-green-01.log](ovrcr-401-single-digit-gui-command-nonjump-green-01.log.gz), and [ovrcr-401-single-digit-gui-focus-loss-green-01.log](ovrcr-401-single-digit-gui-focus-loss-green-01.log.gz).
- Ctrl+Shift+9 does not navigate and ordinary Browse dispatch remains `None`: 1 passed in [ovrcr-401-single-digit-extra-modifier-green-01.log](ovrcr-401-single-digit-extra-modifier-green-01.log.gz).
- Hidden Terminal Ctrl+2 preserves its existing NUL byte: shared terminal encoder unit and public hidden-sidebar action regressions each passed 1/1 in [ovrcr-401-single-digit-ctrl2-terminal-unit-green-02.log](ovrcr-401-single-digit-ctrl2-terminal-unit-green-02.log.gz) and [ovrcr-401-single-digit-hidden-control2-green-04.log](ovrcr-401-single-digit-hidden-control2-green-04.log.gz). The corresponding red reproduction is [ovrcr-401-single-digit-hidden-control2-red-02.log](ovrcr-401-single-digit-hidden-control2-red-02.log.gz).
- Dashboard Agent search/key preservation module passed 31/31: [ovrcr-401-single-digit-agent-search-green-01.log](ovrcr-401-single-digit-agent-search-green-01.log.gz). Operator sidebar visibility and geometry tests passed 1/1 each: [ovrcr-401-single-digit-operator-sidebar-green-01.log](ovrcr-401-single-digit-operator-sidebar-green-01.log.gz) and [ovrcr-401-single-digit-operator-geometry-green-01.log](ovrcr-401-single-digit-operator-geometry-green-01.log.gz).
- GUI binary build passed: [ovrcr-401-single-digit-gui-build-green-01.log](ovrcr-401-single-digit-gui-build-green-01.log.gz). `cargo fmt --all -- --check` and `git diff --check` passed after the last source change and integration merge.

## Intentional expectation changes

- Removed adaptive codes, prefixes, matching-prefix display, and Backspace prefix editing. Replaced adaptive public and compiled acceptance suites with `session_jump_single_digit` coverage.
- Command-digit no longer starts a jump. The revised Control gesture retains Command host shortcuts; the old Command-positive acceptance was recorded as intentionally failing at [ovrcr-401-single-digit-obsolete-command-acceptance-red-01.log](ovrcr-401-single-digit-obsolete-command-acceptance-red-01.log.gz) and its replacement is green above.
- Preserved Ctrl+2's ordinary Terminal NUL encoding through the existing shared control-byte grammar.

## Remaining qualification and owned resources

- The user has not yet physically tried the new native GUI build. Parent will merge/open this exact revision for direct user testing; no screenshot run was requested.
- Full workspace tests and final lint are owned by the parent and have not been run on this new commit. Older baseline full-suite evidence is not attributed to this revision.
- Private build target `/private/tmp/ovrcr-401-build-single-digit` is 2.6 GiB and is handed to the parent for serialized GUI packaging/preview. Tests used private HOME/TMPDIR at `/private/tmp/ovrcr-401-single-digit-home` and `/private/tmp/ovrcr-401-single-digit-tmp`; fixture-owned processes/sockets are expected to be cleaned by their acceptance harnesses.
- Follow-up cleanup: the private HOME and TMPDIR directories listed above were removed after testing. The isolated build target remains available to the parent.

## Integrated gate correction — 2026-11-06

The Agent-search 31/31 run above preceded the final Ctrl+2 alias; it did not qualify that whole module at the final feature commit. The integrated Nextest attempt exposed one stale assertion in `unverified_frontend_keeps_control_hold_and_command_out_of_browse_actions`: it expected ordinary Terminal Ctrl+2 to be dropped instead of encoded as NUL. The application correctly returned `PtyBytes([0])`. The test-only correction `c0829a4b` requires that exact NUL, preserves the no-jump/outbox assertions, and adds explicit unchanged Browse/Terminal mode and focus checks. Fresh Agent-search 31/31 and single-digit 9/9 runs pass on that correction.

The [original full attempt](control-workspace-nextest-01.json) remains failed: 1,152 passed (including one recovered setup retry), one stale assertion failed, 33 skipped and 1,513 not run because of fail-fast. The [resume plan](control-resume-plan.json) covers every remaining selected case: 1,514 require success, including the corrected failure. Rerunning their complete binaries selects 1,551 cases, overlapping 37 earlier passes. No new test is excluded.

The separate [partial-resize runtime case](control-partial-resize-01.json) passed 1/1 under Cargo. The [workspace documentation command](control-doc-tests-01.json) completed successfully with zero defined doctests; it is not behavioral-test evidence. Final resumed and current hosted outcomes are recorded in PR #406.

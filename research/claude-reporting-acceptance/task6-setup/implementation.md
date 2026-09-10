# Task 6 setup and doctor

Implemented `agent setup claude --print [--settings PATH]` and `agent doctor claude --json [--settings PATH] [--session ID] [--executable PATH]`. The global JSON flag is accepted after the doctor subcommand. Setup prints JSON to stdout and review/launcher/removal instructions to stderr. No settings or registry files are written; tests assert original contents and permissions remain intact.

Setup preserves unrelated settings and hook handlers (including empty groups), emits synchronous marked reporters, and is idempotent. Exact legacy `ovrcr report claude-context --stdin-json` and resolved executable forms migrate to the new default statusline reporter. Arbitrary external renderers become one POSIX-quoted `--render-command` argument. Legacy wrappers remain unchanged and produce concrete manual migration instructions. Existing permissions/trust values remain intact. OVRCR handlers are identified by the shell no-op prefix `: ovrcr-managed-claude-v1; ` rather than unsupported custom provider JSON keys.

Doctor reuses the existing one-second bounded version probe. Only the exact supported output yields version 2.1.267; unsupported/unavailable output gives null and remediation. It validates only the supplied file and always says effective enterprise/plugin configuration is unverified. Session inspection uses the existing read-only Inspect path and inherited session ID when provided. Output includes provider/generation, reporter health, separate metric/usage/cost availability and usage coverage, not private conversation/invocation IDs, tokens, transcript paths, raw health reasons, or settings/command contents. Connected transport does not imply known usage or complete accounting.

## Evidence

- attempt01: initial four actual CLI tests passed.
- attempt02: four passed; socket fixture failed with sandbox Operation not permitted (environment denial).
- attempt03: permitted socket test passed; supported executable fixture unexpectedly returned unavailable during the parallel run (four passed, one failed). Existing probe unchanged except pub visibility. Cause not established; no claim this was fixed.
- attempt04: scoped clippy passed.
- attempt05: focused supported/unsupported version recheck passed, one test.
- attempt06: all five CLI tests passed with socket permission.
- attempt07: all five CLI tests passed after preserving empty user hook groups.
- attempt08: final scoped clippy, changed-file formatting and diff checks passed.

Tests execute generated statusline commands, including a copied executable whose pathname contains spaces and an apostrophe; assert external renderer invocation once with original input bytes; check exact versus wrapped legacy migration; inspect real socket fixtures for unbound and unavailable sessions; and ensure secrets/private fields are omitted. These checks do not certify effective native provider configuration or native Claude accounting/completion behavior. Coordinator owns documentation, diary, commit and independent review.


## Review correction: doctor interruption and transient probe investigation

The real CLI interruption regression failed before correction: SIGINT after the fixture probe published its PID/PGID readiness immediately terminated doctor and left that owned probe group alive. The fixture killed its owned leaked group; attempt09 preserves the behavioral failure. Doctor now blocks INT/TERM/HUP/QUIT only during the existing bounded probe and restores the previous mask after probe group cleanup/reap. `src/main.rs -> cli::main -> run -> agent::run -> doctor` starts no other threads before this point, so process-directed signals cannot escape the current-thread mask. The probe inherits the mask, but its existing SIGKILL cleanup is unblockable. On restoration the pending original signal terminates doctor. No shared probe logic changed. Attempt11 passes all four signals; attempt12 records final CLI suite results with each PGID absent by ESRCH before doctor signal exit is accepted.

Reviewed the old false supported-version result against `pinned_version`: it accumulates nonblocking stdout, uses WNOWAIT to retain the leader as the process-group ownership anchor, kills the group, reaps the leader, then drains the remaining bounded pipe tail before exact comparison. No demonstrated output-loss ordering was found. A bounded diagnostic (attempt10) ran 12 sequential and 24 concurrent actual doctor commands with separate fixture startup and post-output completion markers: all 36 returned supported, all markers were present, observed wall time 173–693 ms. This does not establish the original failure's cause. Its unavailable result remains legitimate uncertainty under the one-second probe bound; no claim of fixing that transient or extending the timeout is made.


## Review correction: conservative supplied configuration validation

Foundation review found that doctor accepted reporting command text without checking command-handler type or applicable group matcher, and accepted any statusline suffix after a known prefix. Attempt15 records actual CLI RED for a SessionStart matcher restricted to resume. The correction requires command handlers, an absent/empty/wildcard matcher, and absent/false async values. Statusline must have command type and match either the exact supported default or the precise single-quoted renderer-argument syntax emitted by setup. Other shell syntax remains unverified; no general shell parser was introduced. Effective configuration remains unverified regardless of supplied-file validation.

Attempt16 passes the actual CLI matrix for restricted matcher, wrong hook/statusline types, malformed suffix, wildcard matcher and generated apostrophe-containing renderer. Attempt17 scoped clippy passes; attempt18 records the full seven-test setup/doctor suite. Setup composition semantics are unchanged.

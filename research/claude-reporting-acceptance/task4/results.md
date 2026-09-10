# Task4 observed Claude activity

Base: 3385e6d, codex/claude-reporting. Tests ran against the Task4 working-tree changes delivered with this record. Coordinator-owned Cargo.toml raw_value feature and unwired pure metrics module are excluded from this unit; Task4 does not use that feature/module.

The root private receiver owns current prompt identity and transport activity revision. Only an admitted root can publish. Synchronous UserPromptSubmit establishes the observed prompt boundary; matching tool observations report Busy, matching permission_prompt Notification reports WaitingInput, matching Stop reports Idle/Observed, and matching typed StopFailure reports Error/Observed. Missing/malformed/child/foreign identities and old-prompt terminal observations cannot change the current projection. Startup duplicates cannot overwrite activity. Clear/ambiguous transition remains permanently closed. Failed activity delivery closes the watch and admission; native continues and runtime becomes Unavailable.

Source scope: supported synchronous hooks only; async or arbitrary prompt replay is unsupported. Opaque prompt IDs provide correlation, not source ordering. Transport revisions are not fabricated source sequence. Stop, including stop_hook_active=true, never becomes Confirmed. API failure routing is synthetic-tested but not native-certified. Generic notifications, PermissionRequest alone, and elicitation do not establish human waiting. No metrics, collector, final settling, UI, or setup implementation is included. Legacy unbound/manual reporting behavior remains unchanged.

Native permission evidence: committed attempt-10-sources/live-hooks-and-statusline.jsonl contains one permission_prompt with the same prompt ID as UserPromptSubmit before actual approval. A parser regression consumes that capture. Native prompt/tool/Stop identity evidence also appears in attempt08. The official reference https://code.claude.com/docs/en/hooks documents prompt identity, default synchronous execution, permission notification suppression when answered earlier, and StopFailure API-error semantics; rolling documentation alone is not a native API-failure capture.

Every command below was prefixed with `rtk proxy`. Logs retain stdout/stderr. Socket/PTY tests used required execution permission unless explicitly listed as sandbox denial.

| Attempt | Command | Exit/result |
| --- | --- | --- |
| 01 | cargo test -p ovrcr --test server_lifecycle private_claude_activity_tracks_only_current_root_prompt_observations -- --nocapture | 101, fixture compile error (wrong constructor arguments), not product RED |
| 02 | same | 101, fixture lacked registered project, not product RED |
| 03 | same | 101, actual product RED: UserPromptSubmit left revision0 instead of1; 0 passed/1 failed |
| 04 | same | 0, 1 passed |
| 05 | cargo test -p ovrcr --lib report:: | 101, 8 passed/1 failed: Unix bind Operation not permitted in sandbox |
| 06 | cargo test -p ovrcr --test server_lifecycle agent_admission -- --nocapture | 0, 6 passed/1 ignored helper |
| 07 | cargo test -p ovrcr --lib report:: | 0, 9 passed with socket permission |
| 08 | cargo test -p ovrcr --test server_lifecycle private_claude_activity -- --nocapture | 0, 2 passed: actual hook routing and injected publication failure |
| 09 | cargo test -p ovrcr --test cli agent_hook | 0, 4 passed including empty stdout/fail-open deadline |
| 10 | cargo check -p ovrcr --all-targets --all-features | 0 |
| 11 | cargo clippy -p ovrcr --all-targets --all-features -- -D warnings | 0 |
| 12 | cargo fmt --all -- --check | 0 |
| 13 | git diff --check | 0 |

Actual routing assertions cover root Busy, child/foreign/malformed Stop rejection, duplicate startup, PermissionRequest/generic Notification ignored, correlated Waiting, Stop continuation observed only, later tool Busy, turn B rejecting turn A Stop/Error/wait/tool, missing-turn Stop rejection, current API Error, post-tool failure Busy, and no publication after clear. Failure fixture rejects the bounded report request and verifies Unavailable, same binding generation, no later reopening, and native output through normal exit. Existing closure regressions retain successful-clear lease ownership and lost-Bind/failed-health protections. Linux signal/native acceptance is not claimed by this macOS test slice.

## Review correction: protocol identity validation

Base f608e70. Reviewer traced invalid nonempty prompt IDs entering receiver state before runtime rejected them. The parser now applies shared `validate_agent_id` to session/prompt IDs before mutation; transcript paths retain their separate exact-path semantics. Real routing sends control-character and 257-byte prompt IDs, asserts unchanged activity/revision and Connected health, then accepts later legitimate callbacks. Parser tests cover both fields, accepted 256-byte limits, and paths exceeding 256 bytes.

- 14-review-red.log: `cargo test -p ovrcr --test server_lifecycle private_claude_activity_tracks_only_current_root_prompt_observations -- --nocapture`, exit101, 0 passed/1 failed. Product RED: after malformed prompt, legitimate notification stayed revision1 instead of2.
- 15-review-green.log: `cargo test -p ovrcr --test server_lifecycle private_claude_activity -- --nocapture`, exit0, 2 passed.
- 16-review-parser.log: `cargo test -p ovrcr --lib report::claude::`, exit0, 4 passed.
- 17-review-clippy.log: `cargo clippy -p ovrcr --all-targets --all-features -- -D warnings`, exit0; affected compilation and lint passed.
- 18-review-fmt.log and 19-review-diff.log: `cargo fmt --all -- --check` and `git diff --check`, both exit0.

All commands used `rtk proxy`; real route used required socket/PTY execution permission. No activity mappings or source-certification scope changed.

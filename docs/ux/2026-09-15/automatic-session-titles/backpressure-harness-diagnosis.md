# Backpressure regression readiness failure

Final worktree base: 919f2ec4708fe7676ae46a74149ff0ef334260da plus task changes.

Observed full-suite failure: server_lifecycle.rs:1701 wrote Select during readiness and received Broken pipe. It occurred before Input/SendTerminal backpressure started. Original exact focused run passed 1 test in 1.53s (backpressure-focused.log).

The readiness test was byte-identical to HEAD before this correction. Its loop sent a new Select and consumed only one frame per iteration. server/outbound.rs:243 and :256 enqueue Screen plus Ok for each selected pane. Prior responses are retained (:275-282), and :283 closes the queue when new responses exceed DASHBOARD_QUEUE=64 (server/mod.rs:88). dispatch.rs:616-617 disconnects the dashboard when replace_view fails. Under delayed READY, this test creates more replies than it drains. This is a pre-existing harness defect consistent with the observed close; the exact close branch was not instrumented in the failed full run.

Title-change events do not apply to these explicitly named/pinned blocked and local sessions. No production code changed for this correction.

Correction: send Select once; consume its Screen and Ok plus subsequent output until READY within the same two-second readiness deadline. Consume the acknowledgement explicitly instead of discarding arbitrary frames under a 10ms timeout. All existing backpressure payloads, blocked-response timing assertions, Inspect/Kill assertions, and cleanup remain unchanged; acknowledgement now has an additional assertion.

Verification: CARGO_INCREMENTAL=0 cargo test -p ovrcr --all-features --test server_lifecycle backpressured_input_and_send_do_not_block_inspect_or_kill -- --exact --nocapture, exit 0 (backpressure-harness-fixed.log). Full suite remains parent responsibility.

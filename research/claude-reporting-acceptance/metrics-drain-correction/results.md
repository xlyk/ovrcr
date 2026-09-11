# Final drain correction

Reviewed integration: de80030b5a163882fdd0acf698c01bdacec36f63. Correction tested as working-tree changes; final delivery base 2c432e001ce540d3616e64bf95e5df204afdac99. Concurrent commits changed other workers' files only.

A pending pre-exit EOF response previously stopped final drain before a post-exit request. The receiver now consumes that possible pending response and requires a subsequent response before EOF may stop the drain. The controller permits one request in flight, so the subsequent response belongs to a request issued after native completion. No collector API, native source sequence, completion claim, or deadline changed. The existing two-second absolute deadline retains 700ms for finalization/status recovery.

The deterministic regression uses the real CollectorController and a controlled shell helper speaking its framed protocol. A marker is written after EOF A reaches the helper pipe. Ordinary polling is withheld in the fixture to preserve that exact ordering. A real supervised native child replaces the fixture response source with B, emits POST_EXIT_DRAIN_NATIVE_OUTPUT, and exits17. The actual NativeCompleted callback then drains and sends Finalize through a real Unix socket. The final frame must contain B (30 input tokens), Partial coverage, and native exit remains17 within2.5seconds. This establishes transport ordering; it does not certify native transcript semantics. Existing four real PTY/runtime metrics tests cover the actual reader and routing.

| Attempt | Command (rtk proxy) | Exit/result |
| --- | --- | --- |
| 01 | cargo test -p ovrcr --lib native_completion_drains_past_pending_pre_exit_eof -- --nocapture | 101; fixture failure: accepted socket inherited nonblocking mode; not product RED |
| 02 | same | 101; 1 executed, expected30 but finalization contained10; product RED |
| 03 | same, after fix | 0; 1 passed |
| 04 | cargo test -p ovrcr --lib report:: -- --nocapture | 0; 21 passed |
| 05 | cargo test -p ovrcr --test server_lifecycle claude_metrics_ -- --nocapture | 0; 4 passed |
| 06 | cargo clippy -p ovrcr --all-targets --all-features -- -D warnings | 0 |
| 07 | cargo fmt --all -- --check | 0 |
| 08 | git diff --cached --check | 0 |

All process/socket tests ran with execution permission. Saved attempts retain failures; only trailing whitespace and terminal blank lines are normalized for Git. No native provider or Linux acceptance was run for this bounded correction. Source components remain uncertain/Partial, and EOF is never complete accounting. Coordinator owns independent review and diary.

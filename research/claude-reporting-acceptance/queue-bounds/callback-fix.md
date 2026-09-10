# Exact-size callback correction

The private listener stays nonblocking, but an accepted callback stream must use blocking reads so its existing socket timeouts apply. On macOS, the accepted stream retained nonblocking behavior and could reject a fragmented valid 65,536-byte payload with BrokenPipe at the sender. The fix explicitly sets only that accepted stream to blocking before the unchanged deadline handling.

The two colocated tests use the actual InvocationChannel listener. Exact-limit input must reach the handler once and return its reply. An oversized declared length must close without calling the handler. The limits and existing 200ms input / 800ms handler budgets remain unchanged.

`callback-limit-red.txt` and `callback-limit-green.txt` are summaries of tool-observed focused runs, not raw Cargo stdout. Tested source was 090ca49 plus in-progress Task2 edits, with this accepted-stream fix absent/present respectively. The focused result changed from one pass/one failure to two passes. Other queue instrumentation was present in the checkout and is not included in this correction unit. Full current-revision gates will follow its independent review and the remaining counter work.

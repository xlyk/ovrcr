# Runtime ownership and lifecycle

- Give each piece of state one authority. Avoid parallel stores for selection, geometry, readiness, request completion, or process ownership. Compatibility helpers must delegate to the same implementation.
- Respect the existing lock order and spawn/registration boundary. Keep owner verification and state publication atomic. `Session::spawn_registered` is the only production spawn; it publishes the session before the PTY reader and child waiter start. The two `cfg(test)` hook constructors publish at that same point with a no-op register, so no spawn skips publication. Delayed cleanup from an old connection must leave a replacement owner's state intact.
- Preserve the synchronous dispatcher and sole socket writer. Socket writes must not block the parser dispatcher. Bound queues and retained state; preserve control responses and lifecycle events or disconnect explicitly when they cannot be delivered. `reporting_channel` is the only queue constructor; its monitor argument turns the accounting on for the `acceptance-diagnostics` build and for tests that assert on it, and nothing else varies. The `after_send` / `before_wait` ordering hooks belong to that accounting path and fire only on a monitored channel; an unmonitored one is a plain `mpsc::sync_channel`.
- Apply final PTY output before exit notification. Detach, hide, and pane close change subscriptions; they must not terminate sessions.
- Signal only process groups whose ownership the current runtime or fixture established. A discovered or remembered PID is not ownership proof. Never use broad process-name cleanup.
- Live PTYs and screens belong to the running server. Detach/reattach and explicit relaunch after server loss are different operations; do not infer crash recovery from successful reattachment.
- Preserve atomic persistence and failure boundaries. A failed write must not be reported as committed, and a partial external side effect must not be described as rolled back.

The active dashboard (identity, queue, view, geometry) lives in `server/dashboard.rs`. Prove ownership with `ActiveDashboard::owns`; never compare identities inline.

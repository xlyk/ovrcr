# Final review

Base `ceb8f06d`, reviewed source `a6b508b4`. Independent Standards and Spec reviews.

## Standards

- **Documented architecture mismatch — `src/pi-operator-extension.mjs:4-6`, `src/operator/worker.rs:48-49`.** Both comments say Pi supplies all five tool definitions and their validation. The extension defines `watch_sessions` locally in its `definitions` array; `docs/development/operator.md:26-27` also assigns the watch-status producer to that separate custom tool. Update the comments to distinguish Pi's four native tool factories from the custom watch tool. This conflicts with the repository's requirement to keep implementation guidance aligned with the source architecture (`AGENTS.md`, Maintain).
- **Possible Duplicated Code (judgment call) — `src/report/reporter.rs:183-185`, `208-210`, `242-244`.** `operator_start`, `operator_start_turn`, and `operator_begin_async_turn` repeat the same conversation/session/run identity fence. Centralize that comparison on the run/turn identity types so the authentication check cannot drift; keep the async-wakeup-specific check separate.

## Spec

- **[P2] Interrupted history contradicts leased rollback.** Ticket #469 requires Stop to “roll native model context back to the last completed checkpoint” (ticket line 19; parent spec line 85). Yet `crates/ovrcr-tui/src/dashboard/operator/render.rs:859` renders every `Interrupted` row as “accepted operations may finish; no rollback.” The leased Stop path in `src/operator/worker.rs` restores the checkpoint through `switch_session`, and the native proof confirms a later warm prompt excludes the interrupted prompt and partial reply. Remove or qualify “no rollback” so it distinguishes external accepted operations from model-context rollback; if hard termination differs, reflect that state instead of using one blanket message.

Standards: two findings (one documented ownership mismatch, one duplication judgment). Spec: one P2 interruption-copy finding. All three review findings were corrected in `1dc1e248`; the coordinator verified the correction diff and focused results.

## Existing-data verification

The coordinator also found that schema-32 journals stored `{"Worker":"Completed"}`,
which the new struct variant could not decode. `1dc1e248` adds the known legacy
representation only at the storage reader boundary. The production migration/
open/read regression failed before the fix and passes after it, preserving the
original message bytes and batch digest. See [focused results](review-fixes-evidence.md).

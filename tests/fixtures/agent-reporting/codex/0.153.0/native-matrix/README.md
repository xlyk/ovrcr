# Native source matrix, Codex 0.153.0

Captured from the isolated macOS matrix at OVRCR 033f4ef, using the installed binary pinned by the parent fixture provenance. The original OVRCR GUI was only a native terminal host; current-checkout product and Linux acceptance are unrun. See the [source contract](../../../../../../research/codex-reporting-acceptance/source-contract.md).

JSON snapshots are allowlisted exact-path source records: no user/assistant bodies, environment, account/limits or credentials. Source IDs, numeric components, paths, OS peer/parent IDs and turn ordinals are retained for attribution. `callbacks-main.jsonl` is the main root lifetime; `callbacks.jsonl` is the separate invalid-model lifetime. `invalid-model.json` retains only its public unsupported-model error. `sha256.json` hashes these sanitized artifacts, not secrets. Raw help normalization provenance remains in the parent directory.

Python files are the actual disposable source prototype, not production code or an installable setup. Native prompts are fixed harmless fixture markers. The receiver's OS authentication is macOS-only and its bounded lifecycle/race handling still requires production implementation and tests. It does not answer approvals. Snapshot extraction deliberately omits most source fields; the error was retained separately because the initial allowlist omitted task_complete.error.

Failures preserved: first foreign-first assertion raced after root admission; separate empty-binding synthetic rejection passed. Receiver restart initially hit sandbox bind PermissionError and succeeded after scoped permission. Earlier login/ENOSPC/CUA failures remain in historical reports. Native /context was unrecognized. No additional native attempt was made to hide these failures.

Actual native CUA outcomes and reference values were independently captured by the coordinator in the task's screenshot/accessibility tool trace. Account/limit parts of /status are intentionally not copied. This folder contains source evidence, not screenshots or product test results.

Independent review correction: ongoing source selection is BLOCKED on detectable current-lineage invalidation. Ordinary-turn evidence remains valid. Hard-coded arithmetic/turn assertions do not test replay, ownership, freshness or ordering behavior. The separate foreign-first fixture does exercise the actual prototype rejection path. Cleanup records contain 17 PIDs and 16 groups.

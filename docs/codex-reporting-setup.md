# Codex reporting setup status

OVRCR has no supported managed Codex reporting setup yet. Do not install a reporting hook or use a proposed `ovrcr agent run --provider codex` command based on this investigation. Ordinary native `codex` commands retain their existing behavior. No configuration, trust, approval policy or authentication was changed.

The inspected candidate is exactly Codex CLI 0.153.0. The [source decision](../research/codex-reporting-acceptance/source-contract.md) records two incomplete routes:

- The app-server schema has no passive exact-thread subscribe operation. `thread/read` does not subscribe; using `thread/resume` as telemetry is outside the approved design.
- Root hooks can supply a native rollout path, but invocation-specific root routing, matching header/current paginated lineage and numeric semantics have not been certified. A versioned reader remains possible; this is not a claim that no rollout exists.

Adapter implementation must wait for one safe source route and its native evidence. No source matrix, native status/usage comparison, Linux acceptance, setup command or doctor integration is delivered by the source investigation. There is nothing to remove from user configuration.

Initial explicit resume depends on the same identity proof. Picker/last-session forms, forks, in-process rebinding, Complete final accounting, Confirmed completion, notifications, pricing and universal memory guarantees remain outside this first release. Missing cost and metric components remain unknown.

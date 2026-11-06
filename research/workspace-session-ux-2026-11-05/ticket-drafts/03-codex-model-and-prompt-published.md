## Parent

https://github.com/xlyk/ovrcr/issues/389

## What to build

Use the same session-creation controls to choose a Codex model or Harness default and optionally run an Initial prompt, with the shared reconnect and no-replay behavior.

## Acceptance criteria

- [ ] Implement the proven Codex adapter across capability query, actual UI choices and Server/managed launch; safely reuse native discovery primitives without borrowing delegation authority.
- [ ] Explicit/default model selection works and is remembered after success per project; invalid, changed or unavailable choices refuse without substitution.
- [ ] Blank prompt opens interactive Codex; a nonblank prompt reaches the intended provider through its supported mechanism, preserving configured native arguments and setup/trust boundaries.
- [ ] Use the existing shared launch-intent/run ownership and pending/refused/accepted/unknown semantics; no duplicate dispatch, durable prompt storage or uncertain automatic replay.
- [ ] Configured overrides remain capability-bound to the actual launcher rather than its name; unsupported controls retain useful text and explain why Start cannot proceed.
- [ ] Production Dashboard-to-Server tests cover capability invalidation, option composition, model/default policy, prompt/refusal and the shared reconnect/privacy contract for this adapter; no incompatible wire or settings regression.
- [ ] Record actual installed Codex version, applied options, resolved model/refusal, unique Initial-prompt receipt/executed response, setup interruption and owned cleanup. CUA verifies the Codex UI path; stand-ins alone cannot close this ticket.

## Blocked by

- https://github.com/xlyk/ovrcr/issues/392

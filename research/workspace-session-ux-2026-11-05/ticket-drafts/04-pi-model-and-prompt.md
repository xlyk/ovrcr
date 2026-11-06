## Parent

https://github.com/xlyk/ovrcr/issues/389

## What to build

Choose a Pi model or Harness default and optionally run an Initial prompt through the real creation flow, retaining per-project choices and the shared pending-delivery guarantees.

## Acceptance criteria

- [ ] Implement Pi capability discovery and option composition through the actual UI/Server/managed-launch path, reusing appropriate native primitives without forging delegated-session authority.
- [ ] Show verified Pi choices and explicitly labeled Harness default; reject invalid/stale choices and write project memory only after successful launch.
- [ ] Blank prompt opens interactive Pi; a nonblank Initial prompt reaches the intended provider via its supported mechanism. Preserve native extension/argument boundaries and configured trust behavior.
- [ ] Reuse the shared intent/run and reconciliation contract; reconnect cannot duplicate execution, and restart or delivery uncertainty cannot authorize automatic replay or durable prompt storage.
- [ ] Preserve configured launchers and gate unknown configurations honestly rather than injecting guessed model or prompt arguments.
- [ ] Production-path tests cover Pi discovery/configuration invalidation, explicit/default model application, multiline prompt/refusal, reconnect/privacy and existing reporting/launch behavior.
- [ ] Record actual installed Pi version, applied options, resolved model/refusal, unique prompt receipt/executed response, setup interruption and cleanup; CUA verifies this selection and launch path. Mocks alone do not prove native support.

## Blocked by

- Draft T2: Run a Claude Code Initial prompt safely through reconnects

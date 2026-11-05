## Parent

https://github.com/xlyk/ovrcr/issues/389

## What to build

Use Oh My Pi (omp) as a fully supported model-and-prompt harness in session creation, independently of Pi, with the same per-project defaults and reconnect behavior.

## Acceptance criteria

- [ ] Prove and implement omp-specific discovery, default/explicit model and Initial-prompt contracts; do not infer support from Pi or treat reporting-only support as launch-capability proof.
- [ ] Connect actual omp capability results to UI model choices and the Server/managed invocation, binding them to the resolved launcher and preserving configured arguments/trust.
- [ ] Explicit models and Harness default work without silent substitutions; blank prompt opens interactive use and nonblank prompt is delivered through the supported omp mechanism.
- [ ] Reuse shared project memory, stable launch intent/session-run correlation and pending/refused/accepted/unknown state. Reconnect does not repeat effects; restart does not persist/replay prompt text.
- [ ] Unknown overrides remain available with honest capability limits; native wrapper/separator/extension arguments cannot be confused with model or prompt input.
- [ ] Production Dashboard-to-Server tests cover omp-specific composition/discovery, removed models, setup/refusal, reconnect/privacy and preserved native reporting/session ownership.
- [ ] Record installed omp version, applied options, resolved model/refusal, unique Initial-prompt receipt/executed response, setup interruption and cleanup; CUA verifies the omp UI path. Pi evidence and the prior reviewer-team CLI runs are not acceptance for this feature.

## Blocked by

- Draft T2: Run a Claude Code Initial prompt safely through reconnects

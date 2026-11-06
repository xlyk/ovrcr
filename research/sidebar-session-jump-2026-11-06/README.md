# Sidebar session-jump acceptance

Status: implementation in progress; native GUI acceptance is red at the first checkpoint. No production-terminal compatibility claim.

- Baseline: `8f9569729574b0f0dedb56bd8b9ff1a1384b08fa`.
- [Native GUI first attempt](native-01/review.md): reviewed integration `2ae54eee`, failing Cmd+1 alongside working existing Cmd+K. Original captures and cleanup inventories retained.
- User explicitly selected the existing disposable OVRCR GUI as the computer-use route. Its input must go through the real Dashboard outer PTY and parser. Other terminal hosts remain separate/unverified; iTerm CUA was denied and no alternate control channel was used.
- Automated core/adapter tests do not substitute for native input/visible outcomes. Final corrected captures, integrated regression counts, exact revision and remaining platform gaps will be appended here.

Feature: [#401](https://github.com/xlyk/ovrcr/issues/401), slices [#402](https://github.com/xlyk/ovrcr/issues/402), [#403](https://github.com/xlyk/ovrcr/issues/403), [#404](https://github.com/xlyk/ovrcr/issues/404), [#405](https://github.com/xlyk/ovrcr/issues/405).

## Correction checkpoint

[Native correction review](native-02/review.md), source `8575c5f6`: actual Cmd+1 selection and separate destination marker, retained Browse inspection/no restart, subsequent running marker, and existing Cmd+K/modal priority passed. All owned resources were cleaned up. Native held-label/prefix screenshots remain unverified because the control tool serializes observations after release; timing evidence is retained. Adaptive and Operator integration/final regression remain pending.

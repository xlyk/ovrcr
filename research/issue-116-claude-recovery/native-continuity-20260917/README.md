# Native Claude continuity acceptance — 2026-09-17

**PASS**, tested source `834756356e8fcbeefc78c78c662d6d851be9e75f`: Claude Code 2.1.268 / Haiku 4.5 in the actual macOS GUI, using a disposable OVRCR fixture and isolated provider configuration. The user approved account usage and the provider's OAuth permissions.

A short synthetic marker was supplied in the first prompt. The owned server was shut down and restarted twice; after each restart, the Dashboard's explicit Resume conversation action reopened the same native conversation without a prompt. Only after the second resume did the second user prompt ask for the earlier marker without including its value. Claude recalled it correctly.

The OVRCR row and title remained identical across runs 1, 2, and 3. Before each resume, the durable exact conversation reference was present while attachment was false. Matching callbacks attached each fresh invocation. Process records confirmed both native commands used the exact retained identifier with `--resume` and no prompt argument. Transcript validation found exactly two user messages and zero tool-use or tool-result blocks. An independent requirements reviewer checked the local evidence and found no actionable gap.

## Boundaries

- This proves controlled shutdown/restart and native conversation continuity on macOS. It does not claim a host reboot, uncontrolled crash, or native Linux GUI acceptance.
- Controlled production-PTY tests separately cover restart before a callback, unsupported identity-transition invalidation, missing prerequisites, database failure, capacity and stale ownership. Automated Linux coverage passed in CI.
- Screenshots, accessibility captures, exact identifiers, process inventories and the synthetic transcript were retained locally. They are deliberately excluded from the published summary because the captures include account-session metadata.

## Startup and cleanup

The normal OAuth flow required explicit approval of its scopes. No trust or permission bypass was used; workspace trust applied only to the generated test fixture. The native GUI stopped refreshing after the browser handoff; zooming the owned window restored rendering and input. No production code was changed for that separate helper observation.

After final capture, the replacement server was shut down through its exact socket. The GUI's expected fixture-preservation guard activated because the original server had already exited. The owned GUI was terminated, every recorded PID and owned session process group was verified absent, and the socket was absent before removing the fixture. The disposable provider download/config directory was also removed. No user server was stopped.

All four hosted checks passed on the tested source in [run 35235777003](https://github.com/xlyk/ovrcr/actions/runs/35235777003). No merge or release is implied by this acceptance result.

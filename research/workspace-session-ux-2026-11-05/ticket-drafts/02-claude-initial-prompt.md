## Parent

https://github.com/xlyk/ovrcr/issues/389

## What to build

In the existing Agent form, optionally compose a multiline Initial prompt and run it when starting Claude Code. Keep pending launch state recoverable through Dashboard reconnect, with honest refusal/uncertainty and no automatic replay after a Server restart.

## Acceptance criteria

- [ ] Blank prompt opens interactive use; nonblank Start authorizes execution through the supported Claude mechanism identified in the native contract matrix, preserving native trust/sign-in/approval behavior.
- [ ] Use a real multiline editor: Enter inserts newline, the visible verified submit chord submits once, picker Enter only accepts a choice, paste/cursor/grapheme editing preserves text, and modal bytes/clicks cannot reach the underlying PTY.
- [ ] The Server owns frozen target/options/prompt under a connection-independent accepted launch intent, then binds it to the exact session/run. Repeated accepted intents reconcile status rather than spawn or send again; use the existing lifecycle/mutation owner.
- [ ] A bounded current-Dashboard reconciliation/read path discovers pending intents without the old connection token, distinguishes pending/accepted/refused/unknown state, and exposes recoverable draft text only when non-delivery is known.
- [ ] Keep prompt text in memory for the current launch/run and Server lifetime; clear it at the actual boundary. No ordinary settings, retained metadata, OVRCR Events or diagnostic logs contain it; redact prompt-bearing debug/error paths. Do not add durable prompt storage.
- [ ] Dashboard reconnect retains pending state; Server restart or missing/retired state does not establish non-delivery or trigger resend. Do not treat view readiness, elapsed time, screen text or response-cycle Ready as native prompt acceptance.
- [ ] Production live-server/PTY tests force duplicate Start, lost response, reconnect, stale run, refusal and uncertainty. A unique synthetic marker verifies restart non-replay and absence from OVRCR-owned stores/logs; provider conversation history is a separate contract.
- [ ] Record installed Claude model/prompt receipt or executed response and setup/trust interruption, plus native CUA proof of the editor/chord and cleanup. Gate unsupported launchers honestly while preserving typed text; include shared wire compatibility checks.

## Blocked by

- Draft T1: Choose and launch a verified Claude Code model

# Linux TUI smoke notes for #170 (parent)

Native Codex CUA / Linux TUI verification is for the parent agent after this PR.
This change does not claim those native runs; preserve failed attempts and keep
source evidence separate from synthetic fixtures.

## Supported forms to exercise (isolated managed Codex)

1. Fresh managed Codex (`ovrcr agent run codex -- codex` / Dashboard Agent).
2. Clear / new conversation that emits `SessionStart(source=clear, new id)` →
   Dashboard stays Connected, open Input clears, then Ready / Input alerts attach
   to the new conversation id. Unread is not auto-acked.
3. In-process resume back to an earlier conversation (A→B→A) via
   `SessionStart(source=resume)` → generation advances; late Stop/PermissionRequest
   from B do not Ready/wait on A.
4. Reopened exact `codex resume UUID`: matching `SessionStart(source=resume)`
   attach, then genuine PermissionRequest open/close and Ready without historical
   Ready replay; then a supported clear/resume switch still delivers Input/Ready.
5. `/compact` (or auto-compact) → `SessionStart(source=compact)` same id; binding
   / generation unchanged; no Ready replay.
6. Unsupported fork launch forms / ambiguous conflicting
   `SessionStart(source=startup)` while bound → persistent
   `identity_transition_unavailable` + doctor recovery; Codex process still running.

## Named gap

Native backtracking / fork reuse `SessionStart(source=startup)` and have no
observable invalidation before the next prompt
(`research/codex-reporting-acceptance/history-invalidation.md`). Do not invent
identity from screen text or a later prompt alone; receiving that later startup
freezes reporting rather than claiming continuous tracking during the gap.

## Fixtures vs source

Application tests use the Codex PTY helper synthetic hooks. Live captures remain
under `research/codex-reporting-acceptance/` and
`tests/fixtures/agent-reporting/codex/0.153.0/native-invalidation/`.
Fixture-only callbacks do not close the source-contract gate.

# Linux TUI smoke notes for #169 (parent)

Native Claude CUA / Linux TUI verification is for the parent agent after this PR.
This change does not claim those native runs; preserve failed attempts and keep
source evidence separate from synthetic fixtures.

## Supported forms to exercise (isolated managed Claude)

1. Fresh managed Claude (`ovrcr agent run claude -- claude` / Dashboard Agent).
2. `/clear` → confirm Dashboard stays Connected, open Input clears, then Ready /
   Input alerts attach to the new conversation id.
3. In-process resume back to an earlier conversation (A→B→A) → generation advances;
   late Stop/Notification from B do not Ready/wait on A; Unread is not auto-acked.
4. Foreground `/branch` after leaving the current conversation → reporting follows
   the new id; Ready/Input continue.
5. `/compact` → binding/generation unchanged; no Ready replay.
6. Unsupported bare fork / ambiguous transition → persistent
   `identity_transition_unavailable` + doctor recovery; Claude process still running.

## Named gap

Background fork (`SessionStart(source=fork)` without a leave of the current
foreground conversation) remains an explicit blocker: do not invent identity.

## Fixtures vs source

Application tests use `agent_admission_native_helper` synthetic hooks. Live hook
captures remain under `research/claude-reporting-acceptance/` (attempts 05/07).

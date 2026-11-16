---
name: ovrcr-status
description: Summarize OVRCR inventory, attention items, reporting failures, and live Workspace capacity. Use for an overview, what needs attention, or status of a Project, Workspace, or session.
---

# Inspect OVRCR status

## Installation

Bundled with the matching OVRCR binary for Operator on macOS and Linux; no
personal or Project installation is needed. Use the inherited
`"$OVRCR_EXECUTABLE" --version` and the shared CLI guide. Keep the inherited
instance configuration and socket.

## Usage

1. State the scope. Default to all registered Projects in the current instance,
   even when the Dashboard captured a particular Workspace. Honor an explicitly
   narrower Project, Workspace, or session request; resolve its stable identities
   from current inventory. Ask if the target is ambiguous. The process cwd and
   a later Dashboard selection are not targets.
2. Begin with metadata, using the inherited matching CLI:

   ```sh
   "$OVRCR_EXECUTABLE" --json project list
   "$OVRCR_EXECUTABLE" --json workspace list --project PROJECT
   "$OVRCR_EXECUTABLE" --json terminal list
   ```

   List Workspaces for each Project in scope. Filter session rows by the resolved
   Project/Workspace/session identities, not a display title. Read
   [inventory fields](references/inventory.md) to interpret activity, attention
   and capacity before reporting. Capacity is instance-wide even in a narrower
   report; label it separately.
3. Report inventory and attention: Input requests, Unread results, interrupted or
   exited sessions and ownership/recovery blockers, and unavailable reporting.
   Distinguish observed activity from unknown activity. A Ready observation is
   not task success; quiet output is not inactivity.
4. Read session content only when relevant to the requested inspection, not for
   every inventory row. For a requested reply or blocker inspection, use Claude
   Code/Codex `terminal transcript ID --json`; if an entry is truncated, use
   `terminal read ID --max-lines N`. For Pi/Oh My Pi use `terminal read ID` or
   `terminal screen ID`. Report missing evidence rather than guessing.
5. Finish with the explicit scope, counts, identified attention items and
   reporting gaps, and live Workspace slots used/50 plus the Operator's separate
   slot. A narrower report must not imply that other Projects were checked.

## Read-only boundary

Inspection leaves resources and attention state unchanged. Do not mark Unread
reviewed, answer Input requests, send Paste or Keystrokes, acknowledge stopped
ownership, create or control sessions/Workspaces, or add watches or periodic
checks. Surface suggested next actions for the user instead.

## Failures

If inventory fails, state what could not be inspected; never substitute another
instance or claim an empty/all-clear result. Offline retained rows are not proof
of live capacity or current activity. Preserve Unread, Input requests and
uncertain ownership while reporting the blocker.

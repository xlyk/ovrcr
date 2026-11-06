# Focused Spec review: Control and single-digit delta

Checkout `/private/tmp/ovrcr-401-integration`, frozen head `6095a236f6b1e92e699abec8208f0af58bfb45c2`, baseline `99eb4cee80ec24fc38c518fe9b3191ed9158f1e3`. Read-only; no builds, edits, GUI interaction, or tests.

## Result

No actionable Control/single-digit behavior or scope defect found.

- **Input and allocation match the approved revision.** The shared binding requires the qualified GUI profile and visible sidebar ([keymap.rs](/private/tmp/ovrcr-401-integration/crates/ovrcr-tui/src/dashboard/keymap.rs:436)); capture takes at most nine eligible drawn targets ([state.rs](/private/tmp/ovrcr-401-integration/crates/ovrcr-tui/src/dashboard/state.rs:1034)). Selection accepts only a Control-only digit and treats 0/unmapped digits as retryable without selecting ([state.rs](/private/tmp/ovrcr-401-integration/crates/ovrcr-tui/src/dashboard/state.rs:1263)). This follows the approved passage: “Always show only single ASCII digits `1`–`9`, assigned to at most the first nine eligible drawn Agent title rows” (design § Label allocation, line 41).

- **Shortcut behavior is preserved.** Active non-digit Control shortcuts cancel the hold and fall through to ordinary dispatch ([state.rs](/private/tmp/ovrcr-401-integration/crates/ovrcr-tui/src/dashboard/state.rs:1153)); Ctrl-o remains a Dashboard action except in unobscured Terminal mode, where it becomes PTY input ([state.rs](/private/tmp/ovrcr-401-integration/crates/ovrcr-tui/src/dashboard/state.rs:2008)). The native GUI continues to route Command shortcuts separately. This matches the approved passage: “A non-digit Control shortcut cancels the unfinished gesture and retains its ordinary Dashboard/program handling” (design § Label allocation, line 46). Hidden-sidebar Ctrl+2 is not intercepted by the jump and retains the ordinary NUL encoding specified in the supplied steering.

## Non-blocking documentation note

`docs/development/tui.md:111–112` still says “The shared terminal encoder remains unchanged,” while this delta adds Ctrl+2 → NUL in [key.rs](/private/tmp/ovrcr-401-integration/crates/ovrcr-terminal/src/key.rs:139). The alias is explicitly authorized by the supplied steering and preserves ordinary input; the guide sentence is stale. No other unasked behavior change was found.

## Documentation follow-up

**Resolved** by `a5ecf177de2b3208bd19c2b614920f21d02d4c3e` (`docs(tui): clarify Control byte aliases`) in `/private/tmp/ovrcr-401-single-digit`. The updated [tui.md](/private/tmp/ovrcr-401-single-digit/docs/development/tui.md:111) now says the shared terminal encoder preserves Control-byte aliases, including legacy Ctrl+2-to-NUL behavior. This closes the stale-documentation note; no further review was performed.

## Ctrl+2 gate-test follow-up

**Confirmed; no weakening.** Commit `c0829a4b5a4daa99f105b8c087830eb183399c77` changes only `tests/tui/agent_search.rs`. The unverified-profile case now expects the intentional ordinary `PtyBytes([0])` Ctrl+2 alias, while retaining the no-label/no-hint assertions, Command/Super no-jump assertion, empty outbox, and unchanged focused session. It adds Browse/Terminal footer checks and verifies the footer remains unchanged after the key. This agrees with the approved ordinary-input behavior and the supplied Ctrl+2 preservation contract. Review was read-only; no checks were run.

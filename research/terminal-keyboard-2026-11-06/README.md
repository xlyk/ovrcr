# Regular-terminal keyboard correction

Kyle reported that `:`, `?`, and the new Control gesture did not work in his regular terminal; Space still worked. The current main baseline was `95663626` (PR #406). This follow-up preserves the one-Server/session ownership contracts and changes client input only.

## Reproduction and cause

The compiled Dashboard entered Browse and requested Kitty keyboard flags 11: all keys, event types, and disambiguation. Under that mode a conforming terminal sends unshifted key codes. Without requesting alternate key values, Shift+semicolon and Shift+slash arrived as `;` and `/`, so the `:` and `?` bindings did not match. No keyboard-layout mapping belongs in the Dashboard.

The new real-PTY regression derives input from the flags emitted by the actual Dashboard. Both negotiated shifted-key cases failed on baseline. Plain legacy characters and explicit correctly shifted characters passed, isolating negotiation from key bindings and parsing. Adding alternate-key reporting (flags 15), and matching the GUI mode observer to it, made all four cases pass. The existing push/pop ownership and cleanup assertions remain.

Primary references: [Kitty key-code rules](https://sw.kovidgoyal.net/kitty/keyboard-protocol/#key-codes), [alternate key reporting](https://sw.kovidgoyal.net/kitty/keyboard-protocol/#report-alternate-keys), and [iTerm2 3.7 keyboard settings](https://iterm2.com/3.7/documentation-one-page.html#keys-profile-preferences).

## Native iTerm qualification

Kyle ran a bounded raw-input check in a new iTerm2 tab outside OVRCR. The probe requested flags 15, recorded only the named test keys, popped its mode, and restored the prior user terminal settings. The [native capture](native-iterm2-01.json) reports `TERM_PROGRAM=iTerm.app`, version `3.7.7`, mode 15, shifted colon/question-mark values, standalone Control press/release, and Ctrl+1. This is physical user input, separate from the agent-owned synthetic probe self-tests.

The CLI now recognizes direct macOS iTerm2 3.7.7 as a qualified frontend. SSH, tmux, screen, Zellij and nested OVRCR session markers keep the Standard profile, as do unverified versions. Explicit input-profile selection keeps its original priority. Existing Agent search remains available elsewhere.

The new compiled CLI test failed before that qualification and passed after it, using the captured Control+1 packets through the production environment selector and a real Server. It asserts visible labels, exact target selection, typing only after Control release/readiness, a separate destination acknowledgement, and no token in the other Agent. A proxy case proves inherited iTerm metadata does not enable jumps through tmux. Six keyboard/profile integration cases pass together. A seventh compiled regression preserves a Quick-message draft on Ctrl+Shift+C and still clears it on ordinary Ctrl+C under alternate-key reporting.

Native transport verification does not by itself prove the final Dashboard interaction. The candidate's full native terminal trial, independent review and final checks remain pending at this checkpoint. Screenshots are not required for Kyle's direct trial.

Original red/green runs and the probe self-tests are retained under `/private/tmp/ovrcr-406-input-diagnosis`; the original #406 evidence remains under `research/sidebar-session-jump-2026-11-06`.

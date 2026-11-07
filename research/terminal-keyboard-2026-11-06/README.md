# Regular-terminal keyboard correction

Kyle reported that `:`, `?`, and the new Control gesture did not work in his regular terminal; Space still worked. The current main baseline was `95663626` (PR #406). This follow-up preserves the one-Server/session ownership contracts and changes client input only.

## Reproduction and cause

The compiled Dashboard entered Browse and requested Kitty keyboard flags 11: all keys, event types, and disambiguation. Under that mode a conforming terminal sends unshifted key codes. Without requesting alternate key values, Shift+semicolon and Shift+slash arrived as `;` and `/`, so the `:` and `?` bindings did not match. No keyboard-layout mapping belongs in the Dashboard.

The new real-PTY regression derives input from the flags emitted by the actual Dashboard. Both negotiated shifted-key cases failed on baseline. Plain legacy characters and explicit correctly shifted characters passed, isolating negotiation from key bindings and parsing. Adding alternate-key reporting (flags 15), and matching the GUI mode observer to it, made all four cases pass. The existing push/pop ownership and cleanup assertions remain.

Primary references: [Kitty key-code rules](https://sw.kovidgoyal.net/kitty/keyboard-protocol/#key-codes), [alternate key reporting](https://sw.kovidgoyal.net/kitty/keyboard-protocol/#report-alternate-keys), and [iTerm2 3.7 keyboard settings](https://iterm2.com/3.7/documentation-one-page.html#keys-profile-preferences).

## Native iTerm qualification

Kyle ran a bounded raw-input check in a new iTerm2 tab outside OVRCR. The probe requested flags 15, recorded only the named test keys, popped its mode, and restored the prior user terminal settings. The [native capture](native-iterm2-01.json) reports `TERM_PROGRAM=iTerm.app`, version `3.7.7`, mode 15, shifted colon/question-mark values, standalone Control press/release, and Ctrl+1. This is physical user input, separate from the agent-owned synthetic probe self-tests.

The first candidate recognized direct macOS iTerm2 reporting 3.7.7 as a qualified frontend. Kyle confirmed `:` and `?` worked in the actual updated Dashboard, but Control still did not. Whitelisted process metadata showed that the original tab reported 3.7.2, so the exact-version selector kept it Standard. A fresh-tab check does not establish the metadata of an existing shell.

Kyle then ran the same bounded check in the original tab after detaching OVRCR. Its [separate native capture](native-iterm2-current-tab-01.json) reports version 3.7.2 and independently verifies mode 15, shifted punctuation, standalone Control press/release, and Ctrl+1. Both captures are retained as observed terminal metadata; they are not assertions about an upgraded application's current executable version.

The CLI now recognizes direct macOS iTerm2 reporting either 3.7.2 or 3.7.7. SSH, tmux, screen, Zellij and nested OVRCR session markers keep the Standard profile, as do unverified versions. Explicit input-profile selection keeps its original priority. Existing Agent search remains available elsewhere.

The new compiled CLI test failed before that qualification and passed after it, using the captured Control+1 packets through the production environment selector and a real Server. It asserts visible labels, exact target selection, typing only after Control release/readiness, a separate destination acknowledgement, and no token in the other Agent. A proxy case proves inherited iTerm metadata does not enable jumps through tmux. Six keyboard/profile integration cases pass together. A seventh compiled regression preserves a Quick-message draft on Ctrl+Shift+C and still clears it on ordinary Ctrl+C under alternate-key reporting.

An additional compiled production-environment case for the original tab reproduced the missing labels before the 3.7.2 qualification. The two metadata variants share the same destination/readiness/acknowledgement assertions, and unit classification checks require proxy refusal for both variants.

Native transport verification does not by itself prove the final Dashboard interaction. The first candidate's independent reviews found no actionable issues and hosted checks passed at `01e6c7f8` ([run 42318244388](https://github.com/xlyk/ovrcr/actions/runs/42318244388)). Final native Control navigation, review of the original-tab correction and checks on that revision remain pending at this checkpoint. Screenshots are not required for Kyle's direct trial.

Original red/green runs and the probe self-tests are retained under `/private/tmp/ovrcr-406-input-diagnosis`; the original #406 evidence remains under `research/sidebar-session-jump-2026-11-06`.

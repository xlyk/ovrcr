# Standards review: Control single-digit sidebar jumps

Reviewed frozen revision `6095a236f6b1e92e699abec8208f0af58bfb45c2` against `99eb4cee80ec24fc38c518fe9b3191ed9158f1e3`.

**Verdict: no repo-rule breach; 0 actionable findings; worst severity: none.** The implementation follows the updated Dashboard contract: [state.rs](/private/tmp/ovrcr-401-integration/crates/ovrcr-tui/src/dashboard/state.rs:1035) derives targets from rendered rows and limits labels to nine, as required by [tui.md](/private/tmp/ovrcr-401-integration/docs/development/tui.md:27). The shared keymap retains the Control-hold entry while ordinary Control and Command shortcuts continue through their existing paths. The GUI input encoder tracks physical Control edges and preserves the distinct aggregate macOS Command path. The terminal key encoder's Ctrl+2 NUL mapping preserves the hidden-sidebar input case. Docs and changed assertions reflect the intentional first-nine/scroll behavior.

**Heuristic checked, not raised:** possible `Duplicated Code` in [input.rs](/private/tmp/ovrcr-401-integration/src/gui/input.rs:551): the new Control event arm shares a physical-held/repeat/release shape with the adjacent Super arm:

> Event::Key {
>     key: Key::ControlLeft | Key::ControlRight,
>     pressed,
>     repeat,

Control is physical-only and has its own focus quarantine; Super also accepts aggregate macOS Command and synthetic edges. Extracting a common state machine could hide that meaningful difference, so this is not an actionable smell.

No other listed smell had a concrete maintenance impact in this focused delta. The edits across terminal encoding, Dashboard dispatch, GUI input, tests, and docs are one cohesive input-contract change, not unrelated reasons for change. This was a source review only; runtime and user acceptance are recorded separately by the root reviewer.

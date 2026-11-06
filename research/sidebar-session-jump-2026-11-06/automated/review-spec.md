# SPEC review

Checkout: `/private/tmp/ovrcr-401-integration` at `4e382f24dac95d854d35ec15301b544d229aaaa2`; baseline `8f9569729574b0f0dedb56bd8b9ff1a1384b08fa`. Read-only review; no builds or tests run.

## Findings

- **[P1] Cmd shortcuts are eaten during a jump hold.** The active-gesture fallback in [state.rs](/private/tmp/ovrcr-401-integration/crates/ovrcr-tui/src/dashboard/state.rs:1327) cancels and returns `Redraw` for any non-label Cmd chord, so normal Dashboard/program handling never receives it. The inactive path also swallows Cmd chords in Terminal mode at lines 1160–1168. This conflicts with the spec: “A non-label Cmd shortcut cancels the unfinished gesture and retains its ordinary host/dashboard handling” (design § Label allocation, line 46). `tests/tui/session_jump_adaptive.rs` covers invalid codes and repeats, but not unrelated Cmd shortcut handling.

- **[P1] Hidden-sidebar Cmd+digit is intercepted.** `dispatch_sidebar_jump_key` reserves Cmd+digits even when no jump can start, explicitly including a hidden sidebar (`state.rs:1160–1168`). The spec says labels appear only when the sidebar is visible and “this feature adds no … jump interception” when hidden (design line 15). This prevents the existing host/program behavior for those chords despite no visible destinations.

- **[P2] Availability is not limited to verified terminal setups.** `TerminalGuard::enter_modes` enables Kitty reporting unconditionally (`terminal_guard.rs:65–75`), and `sidebar_jump_can_start` has no verified-host/capability gate (`state.rs:1020–1032`). Thus any host that reports standalone Cmd can show labels even if its Cmd-digit delivery is unverified. The design requires Cmd-hold “only in verified terminal setups” (line 15). The docs explain verification, but do not gate runtime availability; keep actual host qualification open.

## Scope

No unrelated scope creep found. The GUI input forwarding, fixtures, and archived evidence support the required real-Dashboard computer-use workflow. Review did not treat checkpoint/helper evidence as production-host acceptance.

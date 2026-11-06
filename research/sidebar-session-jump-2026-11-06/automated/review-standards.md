# Standards review

Checkout `4e382f24dac95d854d35ec15301b544d229aaaa2` against `8f9569729574b0f0dedb56bd8b9ff1a1384b08fa`; read-only review.

## Documented-standard finding

- **`crates/ovrcr-tui/src/dashboard/state.rs:1149-1168, 2099-2101`; `crates/ovrcr-tui/src/dashboard/hints.rs:4-8`.** The new Command-modifier/digit path is dispatched before the keymap, while its transient labels are separately formatted by `sidebar_jump_footer_hint`. This bypasses [`docs/development/tui.md:48`](docs/development/tui.md), which requires dispatch, footer, key popup, and palette to read one `dashboard::keymap` entry (also [`CONTEXT.md:100-102`](CONTEXT.md)). The new binding therefore has no shared entry for the popup/palette and duplicates its dispatch/hint contract. Add a shared binding representation for this gesture or document a deliberate exception and keep the key surfaces consistent.

## Baseline smell (judgement call)

- **Possible Duplicated Code — `src/gui.rs:32-46` and `50-70`.** `KeyboardEnhancementObserver::process` and `FocusReportingObserver::process` both combine a saved byte tail, scan for enable/disable sequences, update `enabled`, and retain an overlap tail. For example, both use `let mut input = std::mem::take(&mut self.tail); input.extend_from_slice(bytes);` followed by the same scan/retention structure. A shared mode-sequence observer could own that logic.

No other actionable documented-standard breach or baseline smell found. Tool-enforced findings are omitted.

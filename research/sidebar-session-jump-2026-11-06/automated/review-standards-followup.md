# Standards follow-up

Reviewed integrated correction `4efdf48df21707742ff97a8af406ebfab23e71ea` (source correction `f7dbec81`), read-only and limited to the two prior findings.

- **Keymap standard: resolved.** `dashboard::keymap::sidebar_jump_binding` now registers `Action::SidebarJump` in the Global group with its enabled/blocked reason. `state.rs` gates gesture start through `key_binding_for`, and the footer reads the same binding; key-popup and palette coverage was added in `tests/tui/agent_search.rs::shared_jump_binding_appears_in_footer_key_popup_and_palette`.
- **Duplicated observer scan: resolved.** `src/gui.rs` replaces the two identical scanner implementations with one `ModeObserver::process(bytes, enable, disable)`, retaining the longer sequence overlap.

No further issue found within the requested follow-up. Tests were not run.

// Inspect the loaded application manager, never a config file or a new manager.
// Re-sample at each agent_start; session/reporter invalidation must discard the result.
const ESCAPE = "\u001b";
const PREEMPTING_ACTIONS = [
  "app.clipboard.pasteImage",
  "tui.altScreen.search",
  "tui.altScreen.searchNext",
  "tui.altScreen.searchPrevious",
  "tui.altScreen.pageUp",
  "tui.altScreen.pageDown",
  "tui.altScreen.halfPageUp",
  "tui.altScreen.halfPageDown",
  "tui.altScreen.lineUp",
  "tui.altScreen.lineDown",
  "tui.altScreen.previousPrompt",
  "tui.altScreen.nextPrompt",
  "tui.altScreen.top",
  "tui.altScreen.bottom",
];

// Missing optional APIs must never disable ordinary reporting. The caller loads
// this optional module before registering callbacks, preserving report ordering.
export async function loadPiKeybindings() {
  try {
    const api = await import("@earendil-works/pi-tui");
    return typeof api.getKeybindings === "function" ? api.getKeybindings : undefined;
  } catch {
    return undefined;
  }
}

export function inspectPiInterruptBinding(getKeybindings) {
  try {
    if (typeof getKeybindings !== "function") return "Unavailable";
    const manager = getKeybindings();
    if (!manager || typeof manager.getDefinition !== "function"
      || typeof manager.getKeys !== "function" || typeof manager.matches !== "function"
      || !manager.getDefinition("app.interrupt")) return "Unavailable";
    const actions = ["app.interrupt", ...PREEMPTING_ACTIONS];
    for (const action of actions) {
      const keys = manager.getKeys(action);
      if (!Array.isArray(keys) || keys.length > 32
        || keys.some(key => typeof key !== "string" || !key || key.length > 128)) return "Unavailable";
    }
    // The raw byte check uses Pi's own key decoder, including active overrides.
    // searchClose is intentionally omitted: focused overlays can still consume
    // Escape, so byte delivery never establishes turn settlement.
    if (manager.getKeys("app.interrupt").length === 0
      || manager.matches(ESCAPE, "app.interrupt") !== true) return "Unsupported";
    for (const action of PREEMPTING_ACTIONS) {
      const matched = manager.matches(ESCAPE, action);
      if (typeof matched !== "boolean") return "Unavailable";
      if (matched) return "Unsupported";
    }
    return "Escape";
  } catch {
    return "Unavailable";
  }
}

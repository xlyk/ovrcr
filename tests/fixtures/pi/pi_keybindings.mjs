// Explicit fixture public API. This proves production capability plumbing and
// fences, not installed Pi singleton behavior or native interactive handling.
export function getKeybindings() {
  if (process.env.OVRCR_TEST_PI_INTERRUPT_BINDING === "Unavailable") return undefined;
  return {
    getDefinition: action => action === "app.interrupt" ? { action } : undefined,
    getKeys: action => action === "app.interrupt" ? [process.env.OVRCR_TEST_PI_INTERRUPT_BINDING === "Escape" ? "escape" : "ctrl+c"] : [],
    matches: (bytes, action) => action === "app.interrupt" && bytes === "\x1b" && process.env.OVRCR_TEST_PI_INTERRUPT_BINDING === "Escape",
  };
}

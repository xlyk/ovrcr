import { test } from "node:test";
import assert from "node:assert/strict";
import { inspectPiInterruptBinding } from "../src/ovrcr-pi-interrupt.ts";

function loaded(bindings = {}) {
  const active = { "app.interrupt": ["escape"], ...bindings };
  return {
    getDefinition: action => action === "app.interrupt" ? { defaultKeys: "escape" } : undefined,
    getKeys: action => [...(active[action] ?? [])],
    matches: (bytes, action) => bytes === "\u001b" && (active[action] ?? []).includes("escape"),
    set: (action, keys) => { active[action] = keys; },
  };
}

test("only the loaded native Escape binding is supported", () => {
  assert.equal(inspectPiInterruptBinding(() => loaded()), "Escape");
  for (const keys of [[], ["ctrl+c"], ["ctrl+x"]]) {
    assert.equal(inspectPiInterruptBinding(() => loaded({ "app.interrupt": keys })), "Unsupported");
  }
  assert.equal(inspectPiInterruptBinding(() => loaded({ "app.interrupt": ["ctrl+x", "escape"] })), "Escape");
});

test("higher-priority clipboard and fullscreen bindings refuse Escape", () => {
  for (const action of [
    "app.clipboard.pasteImage", "tui.altScreen.search", "tui.altScreen.searchNext",
    "tui.altScreen.searchPrevious", "tui.altScreen.pageUp", "tui.altScreen.pageDown",
    "tui.altScreen.halfPageUp", "tui.altScreen.halfPageDown", "tui.altScreen.lineUp",
    "tui.altScreen.lineDown", "tui.altScreen.previousPrompt", "tui.altScreen.nextPrompt",
    "tui.altScreen.top", "tui.altScreen.bottom",
  ]) {
    assert.equal(inspectPiInterruptBinding(() => loaded({ [action]: ["escape"] })), "Unsupported", action);
  }
  assert.equal(inspectPiInterruptBinding(() => loaded({ "tui.altScreen.searchClose": ["escape"] })), "Escape");
});

test("missing, uninitialized or broken optional APIs produce a bounded unavailable result", () => {
  const broken = [undefined, null, () => null, () => ({}), () => { throw Error("private details"); }];
  for (const getter of broken) assert.equal(inspectPiInterruptBinding(getter), "Unavailable");
  const manager = loaded();
  manager.getDefinition = () => undefined;
  assert.equal(inspectPiInterruptBinding(() => manager), "Unavailable", "TUI fallback is not application initialization");
  for (const value of [null, "escape", [null], [""], ["a".repeat(129)], Array(33).fill("escape")]) {
    const manager = loaded();
    manager.getKeys = () => value;
    assert.equal(inspectPiInterruptBinding(() => manager), "Unavailable");
  }
  const throwing = loaded();
  throwing.matches = () => { throw Error("credential-like error text must not escape"); };
  assert.equal(inspectPiInterruptBinding(() => throwing), "Unavailable");
});

test("each turn samples current loaded bindings; config drift alone has no authority", () => {
  const manager = loaded();
  const get = () => manager;
  assert.equal(inspectPiInterruptBinding(get), "Escape");
  const configOnDisk = { "app.interrupt": [] };
  assert.equal(inspectPiInterruptBinding(get), "Escape", "no file/config input is consulted");
  manager.set("app.interrupt", configOnDisk["app.interrupt"]);
  assert.equal(inspectPiInterruptBinding(get), "Unsupported", "reload applies only when the manager changes");
  manager.set("app.interrupt", ["escape"]);
  assert.equal(inspectPiInterruptBinding(get), "Escape");
});

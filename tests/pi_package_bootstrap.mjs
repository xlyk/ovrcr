import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, writeFileSync, rmSync, symlinkSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import packageEntry from "../pi/extension.ts";
import { initializeOvrcr } from "../src/ovrcr-pi-runtime.ts";

const names = ["ovrcr_capabilities", "ovrcr_start", "ovrcr_inspect", "ovrcr_collect",
  "ovrcr_recover", "ovrcr_checkpoint", "ovrcr_capture_fallback", "ovrcr_recover_fallback",
  "ovrcr_answer_question", "ovrcr_accept", "ovrcr_deliver", "ovrcr_archive"];

async function diagnose({ selection, managed = true, owner, active = names } = {}) {
  const before = { ...process.env };
  const handlers = new Map();
  const notices = [];
  let registrations = 0;
  try {
    delete process.env.OVRCR_AGENT_SOCKET;
    delete process.env.OVRCR_AGENT_TOKEN;
    delete process.env.OVRCR_PI_ENTRY;
    if (managed) Object.assign(process.env, {
      OVRCR_AGENT_SOCKET: "/fixture/not-a-channel", OVRCR_AGENT_TOKEN: "fixture-not-authority",
    });
    if (selection !== undefined) process.env.OVRCR_PI_ENTRY = selection;
    const pi = {
      on(name, callback) { handlers.set(name, callback); },
      registerTool() { registrations += 1; },
      registerCommand() { registrations += 1; },
      getAllTools() { return owner ? names.map(name => ({ name, sourceInfo: { path: owner } })) : []; },
      getActiveTools() { return active; },
    };
    await packageEntry(pi);
    assert.deepEqual([...handlers.keys()], ["session_start"]);
    await handlers.get("session_start")({ type: "session_start", reason: "startup" }, {
      hasUI: true, ui: { notify: (message, level) => notices.push({ message, level }) },
    });
    assert.equal(registrations, 0, "package metadata must never register another Reporter/control owner");
    return notices;
  } finally {
    process.env = before;
  }
}

test("standalone package diagnoses managed launch without manufacturing tools", async () => {
  const notices = await diagnose({ managed: false });
  assert.equal(notices.length, 1);
  assert.match(notices[0].message, /requires a managed Pi launch/);
});

test("credentials without a selected injected entry do not become an owner", async () => {
  assert.match((await diagnose())[0].message, /no selected managed entry/);
});

for (const selection of ["not-json", "{}", '{"schema":2,"path":"/fixture/entry.ts"}',
  '{"schema":1,"path":"relative.ts"}', 'x'.repeat(8193)]) {
  test(`malformed or incompatible owner metadata refuses (${selection.length} bytes)`, async () => {
    assert.match((await diagnose({ selection }))[0].message, /unsupported managed-entry metadata/);
  });
}

test("stale, symlinked and unloaded entry markers cannot certify a factory", async () => {
  const root = mkdtempSync(join(tmpdir(), "ovrcr-package-marker-"));
  try {
    const entry = join(root, "entry.ts");
    const link = join(root, "link.ts");
    const selection = path => JSON.stringify({ schema: 1, path });
    assert.match((await diagnose({ selection: selection(entry) }))[0].message, /entry is missing/);
    writeFileSync(entry, "export default function () {}\n");
    symlinkSync(entry, link);
    assert.match((await diagnose({ selection: selection(link), owner: entry }))[0].message, /entry is missing/);
    assert.match((await diagnose({ selection: selection(entry) }))[0].message, /tools are unavailable/);
    const foreign = join(root, "foreign.ts");
    writeFileSync(foreign, "export default function () {}\n");
    assert.match((await diagnose({ selection: selection(entry), owner: foreign }))[0].message, /tools are unavailable/);
    assert.match((await diagnose({ selection: selection(entry), owner: entry, active: [] }))[0].message, /tools are unavailable or inactive/);
    assert.deepEqual(await diagnose({ selection: selection(entry), owner: entry }), [],
      "a loaded selected inventory needs no warning; authority still belongs to the Server");
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("injected initialization rejects an unbound relative helper binary before registration", async () => {
  const before = { ...process.env };
  let registrations = 0;
  try {
    Object.assign(process.env, { OVRCR_AGENT_SOCKET: "/fixture/not-a-channel", OVRCR_AGENT_TOKEN: "fixture-not-authority" });
    const pi = { on() { registrations += 1; }, registerTool() { registrations += 1; }, registerCommand() { registrations += 1; } };
    await assert.rejects(() => initializeOvrcr(pi, { entry: "injected", binary: "ovrcr" }), /binary path is unavailable/);
    assert.equal(registrations, 0);
  } finally {
    process.env = before;
  }
});

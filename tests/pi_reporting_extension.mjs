import { after, test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, writeFileSync, chmodSync, existsSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { createHost, assistant } from "./fixtures/pi/pi_host.mjs";

const SOURCE = resolve("src/pi-reporting-extension.mjs");
const TRANSPORT = resolve("src/ovrcr-reporting-transport.mjs");
const materialized = [];

after(() => {
  for (const dir of materialized) rmSync(dir, { recursive: true, force: true });
});

function materialize({ hang = false } = {}) {
  const dir = mkdtempSync(join(tmpdir(), "ovrcr-pi-ext-"));
  materialized.push(dir);
  const record = join(dir, "record.jsonl");
  const helper = join(dir, "fake-ovrcr");
  writeFileSync(
    helper,
    `#!/bin/sh\n[ "$1" = report ] && [ "$2" = pi ] && [ "$3" = --stdin ] || exit 9\ncat >> "${record}"; printf '\\n' >> "${record}"\n${hang ? "sleep 5\n" : ""}`,
  );
  chmodSync(helper, 0o700);
  // The receiver materializes the shared transport beside the extension that imports it.
  writeFileSync(join(dir, "ovrcr-reporting-transport.mjs"), readFileSync(TRANSPORT, "utf8"));
  const extension = join(dir, "ovrcr-pi-reporting.mjs");
  writeFileSync(extension, readFileSync(SOURCE, "utf8").replace("__OVRCR_BINARY__", JSON.stringify(helper)));
  return { extension, record };
}

function frames(record) {
  if (!existsSync(record)) return [];
  return readFileSync(record, "utf8").split("\n").filter(Boolean).map((line) => JSON.parse(line));
}

function managed() {
  process.env.OVRCR_AGENT_SOCKET = "/private/ovrcr/socket";
  process.env.OVRCR_AGENT_TOKEN = "f".repeat(64);
}

test("inert outside a managed invocation: registers nothing", async () => {
  delete process.env.OVRCR_AGENT_SOCKET;
  delete process.env.OVRCR_AGENT_TOKEN;
  const { extension } = materialize();
  const host = await createHost(extension);
  assert.deepEqual(host.registered(), []);
});

test("one response cycle: start, end, settled carry run, outcome and ordered sequence", async () => {
  managed();
  const { extension, record } = materialize();
  const host = await createHost(extension, { session: "sess-a" });
  assert.deepEqual(host.registered(), ["agent_end", "agent_settled", "agent_start", "session_shutdown", "session_start"]);
  await host.emit({ type: "session_start", reason: "startup" });
  await host.emit({ type: "agent_start" });
  await host.emit({ type: "agent_end", messages: [{ role: "user", content: "secret prompt" }, assistant("stop")] });
  await host.emit({ type: "agent_settled" });
  const sent = frames(record);
  assert.deepEqual(sent.map((f) => [f.event, f.sequence, f.run, f.outcome]), [
    ["session_start", 1, null, "none"],
    ["agent_start", 2, 1, "none"],
    ["agent_end", 3, 1, "ok"],
    ["agent_settled", 4, 1, "ok"],
  ]);
  for (const frame of sent) {
    assert.equal(frame.schema, 1);
    assert.equal(frame.mode, "tui");
    assert.equal(frame.owner_pid, process.pid);
    assert.equal(frame.session_id, "sess-a");
    assert.match(frame.instance, /^[0-9a-f]{32}$/);
    assert.ok(!JSON.stringify(frame).includes("secret prompt"));
  }
  assert.equal(sent[0].reason, "startup");
});

test("error and aborted outcomes come from the last assistant message; a second run increments", async () => {
  managed();
  const { extension, record } = materialize();
  const host = await createHost(extension);
  await host.emit({ type: "agent_start" });
  await host.emit({ type: "agent_end", messages: [assistant("stop"), assistant("error")] });
  await host.emit({ type: "agent_settled" });
  await host.emit({ type: "agent_start" });
  await host.emit({ type: "agent_end", messages: [assistant("aborted")] });
  await host.emit({ type: "agent_settled" });
  await host.emit({ type: "agent_start" });
  await host.emit({ type: "agent_end", messages: [] });
  await host.emit({ type: "agent_settled" });
  assert.deepEqual(frames(record).map((f) => [f.event, f.run, f.outcome]), [
    ["agent_start", 1, "none"], ["agent_end", 1, "error"], ["agent_settled", 1, "error"],
    ["agent_start", 2, "none"], ["agent_end", 2, "aborted"], ["agent_settled", 2, "aborted"],
    ["agent_start", 3, "none"], ["agent_end", 3, "none"], ["agent_settled", 3, "none"],
  ]);
});

test("continuations inside one cycle keep the run; the last end before settled decides the outcome", async () => {
  managed();
  const { extension, record } = materialize();
  const host = await createHost(extension);
  await host.emit({ type: "agent_start" });
  await host.emit({ type: "agent_end", messages: [assistant("error")] });
  await host.emit({ type: "agent_start" });
  await host.emit({ type: "agent_end", messages: [assistant("stop")] });
  await host.emit({ type: "agent_settled" });
  const sent = frames(record);
  assert.deepEqual(sent.map((f) => f.event), [
    "agent_start", "agent_end", "agent_start", "agent_end", "agent_settled",
  ]);
  for (const frame of sent) assert.equal(frame.run, 1, `continuation changed the run: ${JSON.stringify(frame)}`);
  assert.equal(sent.at(-1).outcome, "ok");
  await host.emit({ type: "agent_start" });
  assert.equal(frames(record).at(-1).run, 2, "a start after settled opens the next cycle");
});

test("non-tui modes never spawn a helper; shutdown reports once then disables", async () => {
  managed();
  const { extension, record } = materialize();
  const host = await createHost(extension, { mode: "rpc" });
  await host.emit({ type: "agent_start" });
  await host.emit({ type: "agent_settled" });
  assert.deepEqual(frames(record), []);
  host.state.mode = "tui";
  await host.emit({ type: "session_shutdown", reason: "reload" });
  await host.emit({ type: "agent_start" });
  assert.deepEqual(frames(record).map((f) => [f.event, f.reason]), [["session_shutdown", "reload"]]);
});

test("a hung helper is killed at the deadline and the handler still resolves", async () => {
  managed();
  const { extension } = materialize({ hang: true });
  const host = await createHost(extension);
  const started = Date.now();
  await host.emit({ type: "agent_start" });
  const elapsed = Date.now() - started;
  assert.ok(elapsed >= 800 && elapsed < 2500, `elapsed ${elapsed}`);
});

test("queue overflow disables reporting and sends one content-free unavailable notice", async () => {
  managed();
  const { extension, record } = materialize({ hang: true });
  const host = await createHost(extension);
  const pending = [];
  for (let i = 0; i < 258; i += 1) pending.push(host.emit({ type: "agent_start" }));
  await Promise.all(pending);
  const sent = frames(record);
  assert.equal(sent.at(-1).event, "unavailable");
  assert.equal(sent.at(-1).reason, "overflow");
  assert.ok(sent.length <= 2, `helpers actually completed: ${sent.length}`);
  await host.emit({ type: "agent_settled" });
  assert.equal(frames(record).length, sent.length, "disabled after overflow");
});

test("a tool error followed by a successful assistant response is a successful cycle", async () => {
  managed();
  const { extension, record } = materialize();
  const host = await createHost(extension);
  await host.emit({ type: "agent_start" });
  await host.emit({
    type: "agent_end",
    messages: [
      assistant("toolUse"),
      { role: "toolResult", isError: true, content: [{ type: "text", text: "secret tool output" }] },
      assistant("stop"),
    ],
  });
  await host.emit({ type: "agent_settled" });
  const sent = frames(record);
  assert.deepEqual(sent.map((f) => [f.event, f.run, f.outcome]), [
    ["agent_start", 1, "none"], ["agent_end", 1, "ok"], ["agent_settled", 1, "ok"],
  ]);
  assert.ok(!readFileSync(record, "utf8").includes("secret tool output"));
});

test("resume and fork announce the conversation without a cycle; a run whose last assistant errored is an error", async () => {
  managed();
  const { extension, record } = materialize();
  const host = await createHost(extension, { session: "sess-resumed" });
  await host.emit({ type: "session_start", reason: "resume", previousSessionFile: "/private/old.jsonl" });
  await host.emit({ type: "agent_start" });
  await host.emit({ type: "agent_end", messages: [assistant("stop"), assistant("error")] });
  await host.emit({ type: "agent_settled" });
  const sent = frames(record);
  assert.deepEqual(sent[0].event, "session_start");
  assert.equal(sent[0].reason, "resume");
  assert.equal(sent[0].run, null);
  assert.ok(!("previousSessionFile" in sent[0]) && !JSON.stringify(sent[0]).includes("old.jsonl"));
  assert.deepEqual(sent.slice(1).map((f) => [f.event, f.run, f.outcome]), [
    ["agent_start", 1, "none"], ["agent_end", 1, "error"], ["agent_settled", 1, "error"],
  ]);
});

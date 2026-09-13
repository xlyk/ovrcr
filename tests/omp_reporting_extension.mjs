import { after, test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, writeFileSync, chmodSync, existsSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { createHost, assistant } from "./fixtures/pi/pi_host.mjs";

const SOURCE = resolve("src/omp-reporting-extension.mjs");
const TRANSPORT = resolve("src/ovrcr-reporting-transport.mjs");
const materialized = [];

after(() => {
  for (const dir of materialized) rmSync(dir, { recursive: true, force: true });
});

function materialize() {
  const dir = mkdtempSync(join(tmpdir(), "ovrcr-omp-ext-"));
  materialized.push(dir);
  const record = join(dir, "record.jsonl");
  const helper = join(dir, "fake-ovrcr");
  writeFileSync(
    helper,
    `#!/bin/sh\n[ "$1" = report ] && [ "$2" = omp ] && [ "$3" = --stdin ] || exit 9\ncat >> "${record}"; printf '\\n' >> "${record}"\n`,
  );
  chmodSync(helper, 0o700);
  // The receiver materializes the shared transport beside the extension that imports it.
  writeFileSync(join(dir, "ovrcr-reporting-transport.mjs"), readFileSync(TRANSPORT, "utf8"));
  const extension = join(dir, "ovrcr-omp-reporting.mjs");
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

test("inert outside a managed invocation and outside the terminal UI", async () => {
  delete process.env.OVRCR_AGENT_SOCKET;
  delete process.env.OVRCR_AGENT_TOKEN;
  const bare = await createHost(materialize().extension);
  assert.deepEqual(bare.registered(), []);
  // In-process task and advisor children share this process with mode "print".
  managed();
  const { extension, record } = materialize();
  const child = await createHost(extension, { mode: "print" });
  await child.emit({ type: "session_start" });
  await child.emit({ type: "agent_start" });
  await child.emit({ type: "agent_end", messages: [assistant("stop")] });
  assert.deepEqual(frames(record), []);
});

test("registers OMP's lifecycle and switch events", async () => {
  managed();
  const host = await createHost(materialize().extension);
  assert.deepEqual(host.registered(), [
    "agent_end",
    "agent_start",
    "session_shutdown",
    "session_start",
    "session_switch",
  ]);
});

test("an end that will continue keeps the cycle open; the final end settles it as observed work", async () => {
  managed();
  const { extension, record } = materialize();
  const host = await createHost(extension, { session: "sess-a" });
  await host.emit({ type: "session_start" });
  await host.emit({ type: "agent_start" });
  await host.emit({ type: "agent_end", messages: [assistant("stop")], willContinue: true });
  await host.emit({ type: "agent_start" });
  await host.emit({ type: "agent_end", messages: [assistant("stop")] });
  assert.deepEqual(frames(record).map((f) => [f.event, f.run, f.outcome, f.will_continue ?? null]), [
    ["session_start", null, "none", null],
    ["agent_start", 1, "none", null],
    ["agent_end", 1, "ok", true],
    ["agent_start", 1, "ok", null],
    ["agent_end", 1, "ok", false],
    ["agent_settled", 1, "ok", null],
  ]);
  for (const frame of frames(record)) {
    assert.equal(frame.schema, 1);
    assert.equal(frame.mode, "tui");
    assert.equal(frame.owner_pid, process.pid);
    assert.equal(frame.session_id, "sess-a");
    assert.match(frame.instance, /^[0-9a-f]{32}$/);
  }
  // A later start after the settled end is a new cycle (a stop-hook continuation).
  await host.emit({ type: "agent_start" });
  assert.equal(frames(record).at(-1).run, 2);
});

test("error and aborted ends settle as error and aborted; no assistant message is none", async () => {
  managed();
  const { extension, record } = materialize();
  const host = await createHost(extension);
  for (const messages of [
    [{ role: "user", content: "secret prompt" }, assistant("error")],
    [assistant("aborted")],
    [],
  ]) {
    await host.emit({ type: "agent_start" });
    await host.emit({ type: "agent_end", messages });
  }
  assert.deepEqual(frames(record).map((f) => [f.event, f.run, f.outcome]), [
    ["agent_start", 1, "none"], ["agent_end", 1, "error"], ["agent_settled", 1, "error"],
    ["agent_start", 2, "none"], ["agent_end", 2, "aborted"], ["agent_settled", 2, "aborted"],
    ["agent_start", 3, "none"], ["agent_end", 3, "none"], ["agent_settled", 3, "none"],
  ]);
  assert.ok(!readFileSync(record, "utf8").includes("secret prompt"));
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
  assert.deepEqual(frames(record).map((f) => [f.event, f.run, f.outcome]), [
    ["agent_start", 1, "none"], ["agent_end", 1, "ok"], ["agent_settled", 1, "ok"],
  ]);
  assert.ok(!readFileSync(record, "utf8").includes("secret tool output"));
});

test("an in-place session switch re-announces the conversation and closes any open cycle", async () => {
  managed();
  const { extension, record } = materialize();
  const host = await createHost(extension, { session: "sess-a" });
  await host.emit({ type: "session_start" });
  await host.emit({ type: "agent_start" });
  host.state.session = "sess-b";
  await host.emit({ type: "session_switch", reason: "resume", previousSessionFile: "/private/a.jsonl" });
  await host.emit({ type: "agent_start" });
  const sent = frames(record);
  assert.deepEqual(sent.map((f) => [f.event, f.session_id, f.run, f.reason ?? null]), [
    ["session_start", "sess-a", null, "startup"],
    ["agent_start", "sess-a", 1, null],
    ["session_start", "sess-b", null, "resume"],
    // Cycle numbering is per instance, not per conversation: the receiver would read a
    // repeated <instance>:<run> as the continuation of the cycle it already published.
    ["agent_start", "sess-b", 2, null],
  ]);
  assert.ok(!JSON.stringify(sent).includes("a.jsonl"));
});

test("shutdown reports once with reason quit and disables", async () => {
  managed();
  const { extension, record } = materialize();
  const host = await createHost(extension);
  await host.emit({ type: "session_shutdown", reason: "replaced" });
  await host.emit({ type: "agent_start" });
  await host.emit({ type: "agent_end", messages: [assistant("stop")] });
  assert.deepEqual(frames(record).map((f) => [f.event, f.reason]), [["session_shutdown", "quit"]]);
});

test("a next start racing an un-awaited end opens a new cycle; the settled end keeps run 1", async () => {
  managed();
  const { extension, record } = materialize();
  const host = await createHost(extension, { session: "sess-a" });
  await host.emit({ type: "agent_start" });
  // Oh My Pi fires agent_end without awaiting it; the next prompt's agent_start can land
  // while the end's helper is still in flight.
  const end = host.emit({ type: "agent_end", messages: [assistant("stop")] });
  await host.emit({ type: "agent_start" });
  await end;
  const sent = frames(record).map((f) => [f.event, f.run]);
  assert.deepEqual(sent, [
    ["agent_start", 1],
    ["agent_end", 1],
    ["agent_settled", 1],
    ["agent_start", 2],
  ]);
});

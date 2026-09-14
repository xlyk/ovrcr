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
  // `hang: "queued"` is a helper that only answers the final drain frame: an ordinary
  // frame stalls before it records anything, exactly as a lost helper does.
  const body = hang === "queued"
    ? `body=$(cat)\ncase "$body" in *session_shutdown*) ;; *) sleep 5;; esac\nprintf '%s\\n' "$body" >> "${record}"\n`
    : `cat >> "${record}"; printf '\\n' >> "${record}"\n${hang ? "sleep 5\n" : ""}`;
  writeFileSync(
    helper,
    `#!/bin/sh\n[ "$1" = report ] && [ "$2" = pi ] && [ "$3" = --stdin ] || exit 9\n${body}`,
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
  assert.deepEqual(host.registered(), ["agent_end", "agent_settled", "agent_start", "session_shutdown", "session_start", "session_tree", "ui_prompt_end", "ui_prompt_start"]);
  assert.deepEqual(host.commands(), ["ovrcr-reattach"]);
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

test("a print-mode shutdown drains nothing and spawns no helper", async () => {
  managed();
  const { extension, record } = materialize();
  const host = await createHost(extension, { mode: "print" });
  await host.emit({ type: "agent_start" });
  await host.emit({ type: "session_shutdown", reason: "quit" });
  assert.equal(existsSync(record), false, "the drain must stay inert outside the terminal UI");
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

test("an outer prompt span opens and closes one Input request; the title never leaves", async () => {
  managed();
  const { extension, record } = materialize();
  const host = await createHost(extension, { session: "sess-a" });
  await host.emit({ type: "agent_start" });
  await host.emit({ type: "ui_prompt_start", reason: "ui_prompt", kind: "select", title: "PROMPT_TITLE_SECRET" });
  await host.emit({ type: "ui_prompt_end", reason: "ui_prompt", kind: "select", title: "PROMPT_TITLE_SECRET" });
  await host.emit({ type: "ui_prompt_start", reason: "ui_prompt", kind: "editor" });
  await host.emit({ type: "ui_prompt_end", reason: "ui_prompt", kind: "editor" });
  const sent = frames(record);
  assert.deepEqual(sent.map((f) => [f.event, f.run, f.kind ?? null]), [
    ["agent_start", 1, null], ["input_open", 1, "select"], ["input_close", 1, null], ["input_open", 1, "editor"], ["input_close", 1, null],
  ]);
  assert.equal(sent[1].request_id, sent[2].request_id);
  assert.notEqual(sent[1].request_id, sent[3].request_id);
  assert.match(sent[1].request_id, /^[0-9a-f]{32}:p1$/);
  assert.ok(!readFileSync(record, "utf8").includes("PROMPT_TITLE_SECRET"));
  assert.ok(!("title" in sent[1]));
});

test("a prompt can open while no cycle is running and never touches the cycle", async () => {
  managed();
  const { extension, record } = materialize();
  const host = await createHost(extension);
  await host.emit({ type: "ui_prompt_start", reason: "ui_prompt", kind: "confirm" });
  await host.emit({ type: "ui_prompt_end", reason: "ui_prompt", kind: "confirm" });
  await host.emit({ type: "agent_start" });
  assert.deepEqual(frames(record).map((f) => [f.event, f.run]), [["input_open", null], ["input_close", null], ["agent_start", 1]]);
});

test("a transition names the conversation it left by id, never by its file path", async () => {
  managed();
  const { extension, record } = materialize();
  const host = await createHost(extension, { session: "sess-b" });
  // Pi names its session files `<timestamp>_<session id>.jsonl`; the binding is the bare
  // session id, so only the trailing id may travel.
  await host.emit({
    type: "session_start", reason: "resume",
    previousSessionFile: "/Users/x/.pi/agent/sessions/--proj--/2026-09-13T10-30-00-000Z_11111111-2222-3333-4444-555555555555.jsonl",
  });
  await host.emit({ type: "session_start", reason: "startup" });
  // A name no id can be read out of is no expectation at all, never a truncation.
  await host.emit({
    type: "session_start", reason: "resume",
    previousSessionFile: `/private/x/${"z".repeat(100)}.jsonl`,
  });
  const sent = frames(record);
  assert.deepEqual(sent.map((f) => f.previous), ["11111111-2222-3333-4444-555555555555", null, null]);
  assert.ok(!readFileSync(record, "utf8").includes("2026-09-13T10-30-00-000Z"), "the timestamp never leaves");
  assert.ok(!readFileSync(record, "utf8").includes(".pi/agent"), "the path never leaves");
});

test("tree navigation invalidates the response cycle and carries only idleness", async () => {
  managed();
  const { extension, record } = materialize();
  const host = await createHost(extension);
  await host.emit({ type: "agent_start" });
  host.state.idle = false;
  await host.emit({ type: "session_tree", node: "SECRET_NODE" });
  host.state.idle = true;
  await host.emit({ type: "session_tree" });
  await host.emit({ type: "agent_start" });
  assert.deepEqual(frames(record).map((f) => [f.event, f.run, f.idle ?? null]), [
    ["agent_start", 1, null],
    ["cycle_invalidated", 1, false],
    ["cycle_invalidated", 1, true],
    ["agent_start", 2, null],
  ]);
  assert.ok(!readFileSync(record, "utf8").includes("SECRET_NODE"));
});

test("the reattach command re-announces a disabled producer once and says so", async () => {
  managed();
  const { extension, record } = materialize();
  const host = await createHost(extension, { session: "sess-a" });
  await host.emit({ type: "agent_start" });
  await host.emit({ type: "session_shutdown", reason: "reload" });
  await host.emit({ type: "agent_start" });
  assert.deepEqual(frames(record).map((f) => f.event), ["agent_start", "session_shutdown"], "disabled");
  await host.run("ovrcr-reattach");
  const sent = frames(record);
  assert.deepEqual(sent.at(-1).event, "session_start");
  assert.deepEqual([sent.at(-1).reason, sent.at(-1).run, sent.at(-1).idle], ["reattach", null, true]);
  assert.deepEqual(host.state.notices, [["OVRCR reporting reattached", "info"]]);
  await host.emit({ type: "agent_start" });
  assert.deepEqual(frames(record).at(-1).event, "agent_start", "reporting resumes after a reattach");
});

test("a shutdown drain discards the queue and delivers only the final frame in one deadline", async () => {
  managed();
  const { extension, record } = materialize({ hang: "queued" });
  const host = await createHost(extension);
  const started = performance.now();
  const queued = [];
  for (let i = 0; i < 5; i += 1) queued.push(host.emit({ type: "agent_start" }));
  await host.emit({ type: "session_shutdown", reason: "reload" });
  const elapsed = performance.now() - started;
  await Promise.all(queued);
  assert.ok(elapsed < 1200, `the drain took ${elapsed}ms: one absolute deadline, not one per queued frame`);
  assert.deepEqual(frames(record).map((f) => f.event), ["session_shutdown"]);
});

test("a replacement factory is a new instance that names the conversation it replaced", async () => {
  managed();
  const { extension, record } = materialize();
  const first = await createHost(extension, { session: "sess-a" });
  await first.emit({ type: "session_start", reason: "startup" });
  await first.emit({ type: "session_shutdown", reason: "resume" });
  const second = await createHost(extension, { session: "sess-b" });
  await second.emit({ type: "session_start", reason: "resume", previousSessionFile: "/private/x/sess-a.jsonl" });
  const sent = frames(record);
  assert.deepEqual(sent.map((f) => [f.event, f.session_id, f.previous ?? null, f.sequence]), [
    ["session_start", "sess-a", null, 1],
    ["session_shutdown", "sess-a", null, 2],
    ["session_start", "sess-b", "sess-a", 1],
  ]);
  assert.notEqual(sent[0].instance, sent[2].instance, "a replaced factory is a new producer");
});

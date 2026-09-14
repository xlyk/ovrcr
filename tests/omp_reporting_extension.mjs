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
  // A child's shutdown drains nothing either: its session id never leaves this process.
  await child.emit({ type: "session_shutdown", reason: "quit" });
  assert.deepEqual(frames(record), []);
  assert.equal(existsSync(record), false, "no helper was spawned for a child session");
});

test("registers OMP's lifecycle and switch events", async () => {
  managed();
  const host = await createHost(materialize().extension);
  // An EXACT list, so a later ticket cannot add a fabricated plugin-refresh or
  // before-switch subscription unnoticed.
  assert.deepEqual(host.registered(), [
    "agent_end",
    "agent_start",
    "session_shutdown",
    "session_start",
    "session_switch",
    "session_tree",
    "tool_approval_requested",
    "tool_approval_resolved",
    "tool_execution_end",
    "tool_execution_start",
  ]);
  // A plugin-resource refresh emits no extension event in Oh My Pi and never re-instantiates
  // a factory; nothing here may pretend otherwise, and a cancellable pre-switch never binds.
  assert.equal(host.registered().includes("session_before_switch"), false);
  assert.equal(host.registered().includes("session_compact"), false);
  assert.deepEqual(host.commands(), ["ovrcr-reattach"]);
});

test("a switch names the conversation it left by id, never by its file path", async () => {
  managed();
  const { extension, record } = materialize();
  const host = await createHost(extension, { session: "sess-a" });
  await host.emit({ type: "session_start" });
  host.state.session = "sess-b";
  await host.emit({
    type: "session_switch", reason: "resume",
    previousSessionFile: "/private/x/2026-09-13T00-00-00-000Z_sess-a.jsonl",
  });
  const sent = frames(record);
  assert.deepEqual(sent.map((f) => [f.event, f.session_id, f.reason, f.previous]), [
    // A startup announcement states no expectation about a previous conversation.
    ["session_start", "sess-a", "startup", null],
    ["session_start", "sess-b", "resume", "sess-a"],
  ]);
  const body = JSON.stringify(sent);
  assert.ok(!body.includes("/private/x"), "no directory leaves the extension");
  assert.ok(!body.includes("2026-09-13T00-00-00-000Z"), "no timestamp leaves the extension");
  assert.ok(!body.includes(".jsonl"));
});

test("a same-file reload is a switch that names the conversation it is already on", async () => {
  managed();
  const { extension, record } = materialize();
  const host = await createHost(extension, { session: "sess-a" });
  await host.emit({
    type: "session_switch", reason: "resume",
    previousSessionFile: "/private/x/2026-09-13T00-00-00-000Z_sess-a.jsonl",
  });
  assert.deepEqual(frames(record).map((f) => [f.event, f.session_id, f.previous]), [
    ["session_start", "sess-a", "sess-a"],
  ]);
});

test("tree navigation invalidates the response cycle and carries only idleness", async () => {
  managed();
  const { extension, record } = materialize();
  const host = await createHost(extension, { session: "sess-a", idle: false });
  await host.emit({ type: "agent_start" });
  await host.emit({
    type: "session_tree", newLeafId: "LEAF_SECRET", oldLeafId: "OLD_LEAF_SECRET",
    summaryEntry: { text: "SUMMARY_SECRET" }, fromExtension: undefined,
  });
  host.state.idle = true;
  await host.emit({ type: "agent_start" });
  const sent = frames(record);
  assert.deepEqual(sent.map((f) => [f.event, f.run, f.idle ?? null]), [
    ["agent_start", 1, null],
    // The frame still names the cycle it abandons, exactly as Pi's does; the receiver
    // publishes no turn for it and forgets the cycle either way.
    ["cycle_invalidated", 1, false],
    // The abandoned cycle is not resumed: the next start opens a new one.
    ["agent_start", 2, null],
  ]);
  const body = JSON.stringify(sent);
  for (const secret of ["LEAF_SECRET", "OLD_LEAF_SECRET", "SUMMARY_SECRET"]) {
    assert.ok(!body.includes(secret), secret);
  }
});

test("the reattach command re-announces this producer once and says so", async () => {
  managed();
  const { extension, record } = materialize();
  const host = await createHost(extension, { session: "sess-a", idle: false });
  await host.emit({ type: "agent_start" });
  await host.run("ovrcr-reattach");
  await host.emit({ type: "agent_start" });
  assert.deepEqual(frames(record).map((f) => [f.event, f.reason ?? null, f.idle ?? null, f.run]), [
    ["agent_start", null, null, 1],
    ["session_start", "reattach", false, null],
    // The reattached producer abandons the cycle it had open; the next start is a new one.
    ["agent_start", null, null, 2],
  ]);
  assert.deepEqual(host.state.notices, [["OVRCR reporting reattached", "info"]]);
});

test("a print-mode reattach delivers nothing, spawns no helper, and says it is unavailable", async () => {
  managed();
  const { extension, record } = materialize();
  const child = await createHost(extension, { mode: "print", session: "child" });
  await child.run("ovrcr-reattach");
  assert.deepEqual(frames(record), []);
  assert.equal(existsSync(record), false, "no helper was spawned for a child session");
  assert.deepEqual(child.state.notices, [["OVRCR reporting is unavailable", "warning"]]);
});

function approval(toolCallId, sessionId = "sess-a") {
  return {
    type: "tool_approval_requested", sessionId, toolName: "bash", toolCallId,
    reason: "APPROVAL_REASON_SECRET", approvalMode: "always-ask",
  };
}
function resolved(toolCallId, approved, sessionId = "sess-a") {
  return {
    type: "tool_approval_resolved", sessionId, toolName: "bash", toolCallId, approved,
    ...(approved ? {} : { reason: "denied by user" }),
  };
}
function askStart(toolCallId, toolName = "ask") {
  return { type: "tool_execution_start", toolCallId, toolName, args: { question: "QUESTION_TEXT_SECRET" }, intent: "ask" };
}
function askEnd(toolCallId, toolName = "ask", isError = false) {
  return { type: "tool_execution_end", toolCallId, toolName, result: { text: "ANSWER_SECRET" }, isError };
}
const requests = (record) => frames(record).map((f) => [f.event, f.namespace ?? null, f.request_id ?? null, f.kind ?? null]);

test("an approval opens and closes one namespaced request and carries no reason", async () => {
  managed();
  const { extension, record } = materialize();
  const host = await createHost(extension, { session: "sess-a" });
  await host.emit(approval("c1"));
  await host.emit(resolved("c1", true));
  assert.deepEqual(requests(record), [
    ["input_open", "approval", "approval:c1", "approval"],
    ["input_close", "approval", "approval:c1", null],
  ]);
  assert.ok(!readFileSync(record, "utf8").includes("APPROVAL_REASON_SECRET"));
});

test("a denial closes the approval exactly as an allow does", async () => {
  managed();
  const { extension, record } = materialize();
  const host = await createHost(extension, { session: "sess-a" });
  await host.emit(approval("c1"));
  await host.emit(resolved("c1", false));
  assert.deepEqual(requests(record).map((r) => r[0]), ["input_open", "input_close"]);
  assert.ok(!readFileSync(record, "utf8").includes("denied by user"));
});

test("an approval from another session id or with none emits nothing", async () => {
  managed();
  const { extension, record } = materialize();
  const host = await createHost(extension, { session: "sess-a" });
  // In-process task and advisor children carry their own session id, or "".
  await host.emit(approval("c1", "other-session"));
  await host.emit(resolved("c1", true, "other-session"));
  await host.emit(approval("c2", ""));
  assert.deepEqual(frames(record), []);
});

test("two approvals overlap: each frame carries its own tool call id", async () => {
  managed();
  const { extension, record } = materialize();
  const host = await createHost(extension, { session: "sess-a" });
  await host.emit(approval("c1"));
  await host.emit(approval("c2"));
  await host.emit(resolved("c1", true));
  await host.emit(resolved("c2", false));
  assert.deepEqual(requests(record).map((r) => [r[0], r[2]]), [
    ["input_open", "approval:c1"],
    ["input_open", "approval:c2"],
    ["input_close", "approval:c1"],
    ["input_close", "approval:c2"],
  ]);
});

test("the ask tool's lifetime is one question request keyed by its tool call id", async () => {
  managed();
  const { extension, record } = materialize();
  const host = await createHost(extension, { session: "sess-a" });
  await host.emit(askStart("q1"));
  await host.emit(askEnd("q1"));
  assert.deepEqual(requests(record), [
    ["input_open", "question", "question:q1", "select"],
    ["input_close", "question", "question:q1", null],
  ]);
  const text = readFileSync(record, "utf8");
  assert.ok(!text.includes("QUESTION_TEXT_SECRET") && !text.includes("ANSWER_SECRET"));
});

test("a tool that is not ask emits nothing", async () => {
  managed();
  const { extension, record } = materialize();
  const host = await createHost(extension, { session: "sess-a" });
  await host.emit(askStart("t1", "bash"));
  await host.emit(askEnd("t1", "bash"));
  assert.deepEqual(frames(record), []);
});

test("an approval opened during a question keeps both requests and their order", async () => {
  managed();
  const { extension, record } = materialize();
  const host = await createHost(extension, { session: "sess-a" });
  await host.emit(askStart("q1"));
  await host.emit(approval("c1"));
  await host.emit(resolved("c1", true));
  await host.emit(askEnd("q1"));
  assert.deepEqual(requests(record).map((r) => [r[0], r[2]]), [
    ["input_open", "question:q1"],
    ["input_open", "approval:c1"],
    ["input_close", "approval:c1"],
    ["input_close", "question:q1"],
  ]);
});

test("an end that failed, timed out or aborted still closes the question", async () => {
  managed();
  const { extension, record } = materialize();
  const host = await createHost(extension, { session: "sess-a" });
  // The extension never reads `isError` or `result`: the tool's end is the end of the
  // wait whatever the outcome was — answered, redirected, cancelled, timed out or errored.
  await host.emit(askStart("q1"));
  await host.emit(askEnd("q1", "ask", true));
  assert.deepEqual(requests(record), [
    ["input_open", "question", "question:q1", "select"],
    ["input_close", "question", "question:q1", null],
  ]);
  const text = readFileSync(record, "utf8");
  assert.ok(!text.includes("isError") && !text.includes("ANSWER_SECRET"));
});

test("an approval requested and resolved in one tick is delivered open before close", async () => {
  managed();
  const { extension, record } = materialize();
  const host = await createHost(extension, { session: "sess-a" });
  // Oh My Pi resolves immediately, without awaiting the request, when the channel cannot
  // prompt: a no-interactive-UI resolution is a closure, never a visible wait. Ordering
  // here rests entirely on the transport's FIFO, so assert it rather than assume it.
  const requested = host.emit(approval("c1"));
  const settled = host.emit(resolved("c1", true));
  await Promise.all([requested, settled]);
  const sent = frames(record);
  assert.deepEqual(
    sent.map((f) => [f.event, f.request_id]),
    [
      ["input_open", "approval:c1"],
      ["input_close", "approval:c1"],
    ],
  );
  assert.ok(sent[0].sequence < sent[1].sequence, "the frames carry ascending sequence");
});

test("an ask execution that starts and ends in one tick is delivered open before close", async () => {
  managed();
  const { extension, record } = materialize();
  const host = await createHost(extension, { session: "sess-a" });
  const started = host.emit(askStart("q1"));
  const ended = host.emit(askEnd("q1", "ask", true));
  await Promise.all([started, ended]);
  const sent = frames(record);
  assert.deepEqual(
    sent.map((f) => [f.event, f.request_id]),
    [
      ["input_open", "question:q1"],
      ["input_close", "question:q1"],
    ],
  );
  assert.ok(sent[0].sequence < sent[1].sequence, "the frames carry ascending sequence");
});

test("a print-mode child emits no approval or question frames", async () => {
  managed();
  const { extension, record } = materialize();
  const child = await createHost(extension, { mode: "print", session: "sess-a" });
  await child.emit(approval("c1"));
  await child.emit(askStart("q1"));
  await child.emit(askEnd("q1"));
  assert.deepEqual(frames(record), []);
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
  await host.emit({
    type: "session_switch", reason: "resume",
    previousSessionFile: "/private/x/2026-09-13T00-00-00-000Z_sess-a.jsonl",
  });
  await host.emit({ type: "agent_start" });
  const sent = frames(record);
  assert.deepEqual(sent.map((f) => [f.event, f.session_id, f.run, f.reason ?? null, f.previous ?? null]), [
    ["session_start", "sess-a", null, "startup", null],
    ["agent_start", "sess-a", 1, null, null],
    ["session_start", "sess-b", null, "resume", "sess-a"],
    // Cycle numbering is per instance, not per conversation: the receiver would read a
    // repeated <instance>:<run> as the continuation of the cycle it already published.
    ["agent_start", "sess-b", 2, null, null],
  ]);
  assert.ok(!JSON.stringify(sent).includes(".jsonl"));
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

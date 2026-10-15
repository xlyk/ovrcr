import { test } from "node:test";
import assert from "node:assert/strict";
import { createDelegationContinuation, validatePointer } from "../src/ovrcr-delegation-continuation.mjs";

const pointer = { schema: 1, instance: "instance", workflow: "workflow", version: 1, digest: "a".repeat(64) };
const snapshot = () => ({ authority: { workflow: "workflow", instance: "instance", epoch: 4, producer: "public-producer" }, active: true, handles: ["prepared", "survivor"], pointer: { ...pointer }, continuation: { objective: "objective-v1.json", decisions: ["decision-v1.json"] }, gaps: [] });
function fixture(call, entries = [{ type: "custom", customType: "ovrcr_workflow_pointer", data: { ...pointer } }]) {
  const hooks = new Map(), appended = [], restored = [], resets = [];
  const pi = { on(name, handler) { hooks.set(name, handler); }, appendEntry(customType, data) { appended.push({ customType, data }); } };
  const ctx = { sessionManager: { getBranch: () => entries } };
  const continuation = createDelegationContinuation({ pi, call, auth: () => ({ source: "memory-only" }), restored: (state, options) => { restored.push(state); resets.push(options?.fresh === true); } });
  return { continuation, appended, restored, resets, emit: (type, event = {}) => hooks.get(type)({ type, ...event }, ctx), ctx };
}

test("actual startup and both compaction outcomes reconcile the original workflow without effects", async () => {
  const requests = [];
  const f = fixture(async request => { requests.push(request); return [snapshot()]; });
  await f.emit("session_start", { reason: "startup" });
  await f.emit("session_before_compact");
  await f.emit("session_compact");
  await f.emit("session_compact_failed");
  assert.equal(requests.length, 3);
  assert.ok(requests.every(request => Object.keys(request).join() === "Recover"));
  assert.ok(requests.every(request => request.Recover.workflow === "workflow"));
  assert.equal(f.continuation.state.authority.epoch, 4, "compaction never transfers the controller");
  assert.deepEqual(f.continuation.state.handles, ["prepared", "survivor"]);
  const message = await f.emit("before_agent_start");
  assert.match(message.message.content, /objective-v1.json/);
  assert.match(message.message.content, /decision-v1.json/);
  assert.ok(!JSON.stringify(f.appended).includes("memory-only"), "native pointers contain no source capability");
});

for (const reason of ["fork", "new"]) test(`${reason} cannot inherit a copied pointer`, async () => {
  let calls = 0;
  const f = fixture(async () => { calls++; return [snapshot()]; });
  await f.emit("session_start", { reason });
  assert.equal(calls, 0);
  assert.deepEqual(f.resets, [true]);
  assert.equal(f.continuation.state, null);
  assert.match(f.continuation.gap, /grant no control/);
});

test("malformed or overflowing pointers report a gap without contacting control", async () => {
  let calls = 0;
  const entries = [{ type: "custom", customType: "ovrcr_workflow_pointer", data: { ...pointer, source: "must-not-be-a-pointer-field" } }];
  const f = fixture(async () => { calls++; return []; }, entries);
  await f.emit("session_start", { reason: "startup" });
  assert.equal(calls, 0);
  assert.match(f.continuation.gap, /Malformed/);
  assert.throws(() => validatePointer({ ...pointer, version: Number.MAX_SAFE_INTEGER + 1 }), /Malformed/);
  assert.throws(() => validatePointer({ ...pointer, digest: "truncated" }), /Malformed/);
});

test("failed publication preserves original pointer and unresolved assignments", async () => {
  const f = fixture(async request => {
    if (request.Save) throw new Error("private storage is full");
    return [snapshot()];
  });
  await f.emit("session_start", { reason: "startup" });
  const old = structuredClone(f.continuation.state);
  await assert.rejects(f.continuation.save({ schema: 1 }), /storage is full/);
  assert.deepEqual(f.continuation.state, old);
  assert.equal(f.appended.length, 1);
});

test("a late startup reply cannot restore a retired factory", async () => {
  let release;
  const f = fixture(() => new Promise(resolve => { release = resolve; }));
  const pending = f.emit("session_start", { reason: "startup" });
  await f.emit("session_shutdown", { reason: "reload" });
  release([snapshot()]);
  await pending;
  assert.equal(f.continuation.state, null);
  assert.match(f.continuation.gap, /retired/);
  assert.equal(f.appended.length, 0);
});

test("current continuation and scoped legacy gaps both reach startup context", async () => {
  const gaps = [{ workflow: "legacy", identity: { request: "original-request" }, brief: "/original/brief-v1.json", reason: "missing certified authority" }];
  const f = fixture(async () => ({ workflows: [snapshot()], gaps }));
  await f.emit("session_start", { reason: "startup" });
  const context = (await f.emit("before_agent_start")).message.content;
  assert.match(context, /objective-v1.json/);
  assert.match(context, /survivor/);
  assert.match(context, /original-request/);
  assert.match(context, /missing certified authority/);
  assert.equal(f.continuation.state.authority.epoch, 4);
});
test("a pointer with only legacy gaps preserves the real gap without invented authority", async () => {
  const f = fixture(async () => ({ workflows: [], gaps: [{ workflow: "workflow", reason: "transfer unavailable for older record" }] }));
  await f.emit("session_start", { reason: "startup" });
  assert.equal(f.continuation.state, null);
  assert.match(f.continuation.gap, /transfer unavailable/);
  assert.doesNotMatch(f.continuation.gap, /TypeError|undefined/);
  assert.match((await f.emit("before_agent_start")).message.content, /transfer unavailable/);
});

test("explicit new retires an older recovery reply without clearing the new context", async () => {
  let release;
  const f = fixture(() => new Promise(resolve => { release = resolve; }));
  const old = f.emit("session_start", { reason: "startup" });
  await f.emit("session_start", { reason: "new" });
  release([snapshot()]);
  await old;
  assert.deepEqual(f.resets, [true]);
  assert.equal(f.continuation.state, null);
  assert.match(f.continuation.gap, /grant no control/);
  assert.equal(f.appended.length, 0);
});

test("ordinary failed or ambiguous recovery never asks for fresh logical routing", async () => {
  for (const reply of [new Error("Server lifetime changed"), [snapshot(), snapshot()]]) {
    const f = fixture(async () => { if (reply instanceof Error) throw reply; return reply; });
    await f.emit("session_start", { reason: "startup" });
    assert.deepEqual(f.resets, [false]);
    assert.equal(f.continuation.state, null);
    assert.ok(f.continuation.gap);
  }
});

import { test } from "node:test";
import assert from "node:assert/strict";
import { createDelegationObserver } from "../src/ovrcr-delegation-observer.mjs";

function driver(t, { failures = 0 } = {}) {
  const messages = [];
  const calls = [];
  const pending = [];
  let messageWaiter;
  let callWaiter;
  const observer = createDelegationObserver({
    pi: { sendMessage(message, options) { if (failures-- > 0) throw new Error("delivery failed"); messages.push({ message, options }); messageWaiter?.(); messageWaiter = undefined; } },
    auth: () => ({ session: 1, capability: "fixture" }),
    call(request, { signal } = {}) {
      calls.push(request);
      callWaiter?.(); callWaiter = undefined;
      return new Promise((resolve, reject) => {
        let entry;
        const done = (error, value) => {
          signal?.removeEventListener("abort", abort);
          const index = pending.indexOf(entry);
          if (index >= 0) pending.splice(index, 1);
          error ? reject(error) : resolve(value);
        };
        const abort = () => done(new DOMException("wait cancelled", "AbortError"));
        entry = { resolve: value => done(null, value), reject: error => done(error) };
        pending.push(entry);
        signal?.addEventListener("abort", abort, { once: true });
        if (signal?.aborted) abort();
      });
    },
  });
  t.after(() => observer.stop());
  return { observer, messages, calls, push: batch => pending.shift().resolve(batch), fail: () => pending.shift().reject(new Error("disconnected")), nextMessage: () => new Promise(resolve => { messageWaiter = resolve; }), nextCall: () => new Promise(resolve => { callWaiter = resolve; }) };
}
const batch = (observations) => ({ cursor: { server_epoch: "server", sequence: 1 }, status: "Changed", progress_deadline_ms: 1000, observations });
const item = (handle, digest = "digest") => ({ handle, current: true, activity: { state: "Busy" }, input_requests: [], publication: { identity: { assignment: { task: handle }, kind: "Submission", version: 1, digest }, handled: false }, stalled: false });

test("content-free publication wake uses Pi followUp API and coalesces outstanding work", async t => {
  const d = driver(t);
  d.observer.watch("a");
  let message = d.nextMessage();
  d.push(batch([item("a")]));
  await message;
  assert.equal(d.messages[0].message.customType, "ovrcr_wake");
  assert.equal(d.messages[0].message.display, false);
  assert.deepEqual(d.messages[0].options, { triggerTurn: true, deliverAs: "followUp" });
  assert.deepEqual(d.messages[0].message.details.handles, ["a"]);
  assert(!d.messages[0].message.content.includes("digest"));
  await d.nextCall();
  d.observer.watch("b");
  d.push(batch([item("a"), item("b")]));
  await d.nextCall();
  assert.equal(d.messages.length, 1, "only one continuation outstanding");
  d.observer.acknowledge();
  assert.equal(d.messages.length, 2);
  assert.deepEqual(d.messages[1].message.details.handles, ["b"]);
  d.push(batch([item("a"), { ...item("b"), publication: { ...item("b").publication, handled: true } }]));
  await d.nextCall();
  d.observer.acknowledge();
  assert.equal(d.messages.length, 2, "duplicate/handled results do not queue another continuation");
});

test("observer reconnect reconciles unhandled publications without treating wake as handling", async t => {
  const d = driver(t);
  d.observer.watch("a");
  let message = d.nextMessage();
  d.push(batch([item("a")]));
  await message;
  d.observer.reconnect();
  message = d.nextMessage();
  d.push(batch([item("a")]));
  await message;
  assert.equal(d.messages.length, 2);
});

test("wait AbortSignal cancels observation only and disconnection wake carries no error body", async t => {
  const d = driver(t);
  const controller = new AbortController();
  const waiting = d.observer.wait({ handles: ["a"], wait_ms: 1000 }, controller.signal);
  controller.abort();
  await assert.rejects(waiting, { name: "AbortError" });
  assert(d.calls.every(call => call.Observe), "no task-control mutation is sent");
  d.observer.watch("b");
  const message = d.nextMessage();
  d.fail();
  await message;
  assert.equal(d.messages[0].message.details.status, "disconnected");
  assert(!d.messages[0].message.content.includes("error"));
});

test("65 pending identities drain in bounded batches without a new state change", async t => {
  const d = driver(t);
  d.observer.watch("first");
  const first = d.nextMessage(); d.push(batch([item("first")])); await first;
  await d.nextCall();
  const handles = Array.from({ length: 65 }, (_, i) => `h${i}`);
  for (const handle of handles) d.observer.watch(handle);
  d.push(batch(handles.slice(0, 64).map(handle => item(handle)))); await d.nextCall();
  d.push(batch([item(handles[64])])); await d.nextCall();
  assert.equal(d.messages.length, 1);
  d.observer.acknowledge();
  assert.equal(d.messages[1].message.details.handles.length, 64);
  d.observer.acknowledge();
  assert.deepEqual(d.messages[2].message.details.handles, ["h64"]);
  assert.equal(new Set(d.messages.slice(1).flatMap(m => m.message.details.handles)).size, 65);
});

test("synchronous Pi delivery failure retains identities for retry", async t => {
  const d = driver(t, { failures: 1 });
  d.observer.watch("a");
  d.push(batch([item("a")])); await d.nextCall();
  assert.equal(d.messages.length, 0);
  const retry = d.nextMessage();
  d.push(batch([item("a")])); await retry;
  assert.deepEqual(d.messages[0].message.details.handles, ["a"]);
});

test("handled publication suppresses its old Ready after reconnect", async t => {
  const d = driver(t);
  d.observer.watch("a");
  const handled = { ...item("a"), activity: { state: "ResponseReady", quality: "Confirmed" }, publication: { ...item("a").publication, handled: true } };
  d.push(batch([handled])); await d.nextCall();
  assert.equal(d.messages.length, 0);
  d.observer.reconnect();
  d.push(batch([handled])); await d.nextCall();
  assert.equal(d.messages.length, 0);
});


test("handling result1 never suppresses a new result in the same contract", async t => {
  const d = driver(t);
  d.observer.watch("a");
  const old = item("a");
  old.publication.handled = true;
  d.push(batch([old])); await d.nextCall();
  assert.equal(d.messages.length, 0);
  const next = item("a", "digest2");
  next.publication.identity.version = 2;
  assert.deepEqual(next.publication.identity.assignment, old.publication.identity.assignment);
  const message = d.nextMessage();
  d.push(batch([next])); await message;
  assert.deepEqual(d.messages[0].message.details.handles, ["a"]);
  assert(!d.messages[0].message.content.includes("digest2"));
});

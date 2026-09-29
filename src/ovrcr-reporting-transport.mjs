// Shared bounded delivery for OVRCR reporting extensions (Pi, Oh My Pi). Materialized
// beside each provider extension for one managed invocation. Captures identity and
// ordering synchronously, spawns the helper directly (no shell) so the channel's
// native-parent check holds, keeps one helper in flight, and never carries content.
import { spawn } from "node:child_process";
import { randomBytes } from "node:crypto";

export function outcomeOf(messages) {
  for (let i = messages.length - 1; i >= 0; i -= 1) {
    const message = messages[i];
    if (message && message.role === "assistant") {
      if (message.stopReason === "error") return "error";
      if (message.stopReason === "aborted") return "aborted";
      return "ok";
    }
  }
  return "none";
}

/**
 * The conversation a transition left, as its identity alone. Both harnesses name a session
 * file `<timestamp>_<session id>.jsonl` while binding the bare session id, so this takes the
 * trailing id and nothing else — never the timestamp, never the directory, never anything
 * inside the file. Anchored, so a name no bounded id can be read out of yields null (no
 * expectation) instead of a truncation that would contradict a healthy binding.
 */
export function idOf(file) {
  return file?.match(/(?:^|[/_])([A-Za-z0-9][A-Za-z0-9.-]{0,63})\.jsonl$/)?.[1] ?? null;
}

/**
 * The model identity alone, as the session reports it. Rejects empty, oversized, or
 * control-bearing values so a bad model never reaches the helper as a made-up label.
 */
export function modelId(model) {
  const id = typeof model?.id === "string" ? model.id.trim() : "";
  if (!id || id.length > 256 || /[\u0000-\u001f\u007f]/.test(id)) return null;
  return id;
}

export function createReporter({
  binary,
  helperArgs,
  deadlineMs = 900, // inside the channel's one-second callback budget
  maxEvents = 256,
  maxBytes = 1024 * 1024,
}) {
  const producer = {
    instance: randomBytes(16).toString("hex"),
    sequence: 0,
    run: 0,
    prompt: 0,
    open: false,
    outcome: "none",
    disabled: false,
  };
  const queue = [];
  let queuedBytes = 0;
  let draining = Promise.resolve(false);
  // Test hook: the helper for exactly this sequence number is never spawned, simulating a
  // helper lost at its deadline. Inert unless the variable is set.
  const dropSequence = Number(process.env.OVRCR_TEST_DROP_SEQUENCE ?? 0);

  // Identity and ordering are captured here, synchronously, before any delivery.
  function frame(event, ctx, extra) {
    producer.sequence += 1;
    return JSON.stringify({
      schema: 1,
      event,
      mode: ctx.mode,
      owner_pid: process.pid,
      instance: producer.instance,
      sequence: producer.sequence,
      session_id: ctx.sessionManager.getSessionId(),
      session_file: ctx.sessionManager.getSessionFile() ?? null,
      run: producer.run > 0 ? producer.run : null,
      outcome: producer.outcome,
      ...extra,
    });
  }

  function deliver(body) {
    return new Promise((resolve) => {
      let child;
      try {
        child = spawn(binary, helperArgs, { stdio: ["pipe", "ignore", "ignore"] });
      } catch {
        resolve(false);
        return;
      }
      const timer = setTimeout(() => child.kill("SIGKILL"), deadlineMs);
      child.once("error", () => {
        clearTimeout(timer);
        resolve(false);
      });
      child.once("close", (code) => {
        clearTimeout(timer);
        resolve(code === 0);
      });
      child.stdin.once("error", () => {});
      child.stdin.end(body);
    });
  }

  // One helper in flight; FIFO; bounded. Overflow disables this producer after one
  // content-free notice; the receiver pauses on it, and `reattach` below re-enables it.
  function report(event, ctx, extra = {}) {
    if (producer.disabled || ctx.mode !== "tui") return Promise.resolve(false);
    const body = frame(event, ctx, extra);
    if (dropSequence && producer.sequence === dropSequence) return Promise.resolve(false);
    if (queue.length >= maxEvents || queuedBytes + body.length > maxBytes) {
      producer.disabled = true;
      queue.length = 0;
      queuedBytes = 0;
      const notice = frame("unavailable", ctx, { reason: "overflow" });
      draining = draining.then(() => deliver(notice));
      return draining;
    }
    queue.push(body);
    queuedBytes += body.length;
    const mine = draining.then(() => {
      const next = queue.shift();
      if (next === undefined) return false;
      queuedBytes -= next.length;
      return deliver(next);
    });
    draining = mine.catch(() => false);
    return mine;
  }

  // One absolute deadline for a shutdown or reload drain: the queue is discarded and only
  // this final frame is delivered, inside the one remaining helper budget rather than one
  // budget per frame still waiting.
  function flush(event, ctx, extra = {}) {
    if (ctx.mode !== "tui") return Promise.resolve(false);
    queue.length = 0;
    queuedBytes = 0;
    const body = frame(event, ctx, extra);
    draining = draining.then(() => deliver(body));
    return draining;
  }

  // Re-announce this producer from inside the already-loaded extension: the disabled flag
  // and the queue are cleared and one session_start-shaped frame is delivered directly.
  // The instance and its sequence continue, so the receiver still fences what came before.
  function reattach(ctx, extra = {}) {
    if (ctx.mode !== "tui") return Promise.resolve(false);
    producer.disabled = false;
    queue.length = 0;
    queuedBytes = 0;
    producer.run = 0;
    producer.open = false;
    producer.outcome = "none";
    const body = frame("session_start", ctx, { reason: "reattach", ...extra });
    draining = draining.then(() => deliver(body));
    return draining;
  }

  return { producer, report, flush, reattach };
}

// OVRCR reporting for a managed interactive Pi session. Loaded only by
// `ovrcr agent run pi` through an explicit -e path beside the user's own extensions;
// inert without the private invocation channel. Sends bounded identifiers and
// discriminants to a directly spawned helper, never prompts, responses or tool data.
import { spawn } from "node:child_process";
import { randomBytes } from "node:crypto";

// Replaced with the JSON-encoded absolute path of the supervising ovrcr binary when the
// receiver materializes this file for one invocation.
const OVRCR_BINARY = __OVRCR_BINARY__;
const HELPER_ARGS = ["report", "pi", "--stdin"];
const HELPER_DEADLINE_MS = 900; // inside the channel's one-second callback budget
const MAX_QUEUED_EVENTS = 256;
const MAX_QUEUED_BYTES = 1024 * 1024;

export default function (pi) {
  if (!process.env.OVRCR_AGENT_SOCKET || !process.env.OVRCR_AGENT_TOKEN) return;
  const producer = {
    instance: randomBytes(16).toString("hex"),
    sequence: 0,
    run: 0,
    outcome: "none",
    disabled: false,
  };
  const queue = [];
  let queuedBytes = 0;
  let draining = Promise.resolve(false);

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
      run: producer.run > 0 ? producer.run : null,
      outcome: producer.outcome,
      ...extra,
    });
  }

  function deliver(body) {
    return new Promise((resolve) => {
      let child;
      try {
        child = spawn(OVRCR_BINARY, HELPER_ARGS, { stdio: ["pipe", "ignore", "ignore"] });
      } catch {
        resolve(false);
        return;
      }
      const timer = setTimeout(() => child.kill("SIGKILL"), HELPER_DEADLINE_MS);
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
  // content-free notice (recovery is a later ticket).
  function report(event, ctx, extra = {}) {
    if (producer.disabled || ctx.mode !== "tui") return Promise.resolve(false);
    const body = frame(event, ctx, extra);
    if (queue.length >= MAX_QUEUED_EVENTS || queuedBytes + body.length > MAX_QUEUED_BYTES) {
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

  function outcomeOf(messages) {
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

  pi.on("session_start", (event, ctx) => {
    producer.run = 0;
    producer.outcome = "none";
    return report("session_start", ctx, { reason: event.reason });
  });
  pi.on("agent_start", (_event, ctx) => {
    producer.run += 1;
    producer.outcome = "none";
    return report("agent_start", ctx);
  });
  pi.on("agent_end", (event, ctx) => {
    producer.outcome = outcomeOf(event.messages ?? []);
    return report("agent_end", ctx);
  });
  pi.on("agent_settled", (_event, ctx) => report("agent_settled", ctx));
  pi.on("session_shutdown", (event, ctx) => {
    const sent = report("session_shutdown", ctx, { reason: event.reason });
    producer.disabled = true; // Pi re-runs factories after replacement or reload
    return sent;
  });
}

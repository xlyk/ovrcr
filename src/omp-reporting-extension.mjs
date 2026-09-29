// OVRCR reporting for a managed interactive Oh My Pi session. Loaded only by
// `ovrcr agent run omp` through an explicit -e path beside the user's own extensions;
// inert without the private invocation channel and outside the terminal UI (task and
// advisor sessions run in this same process with mode "print"). Oh My Pi has no settled
// event: an agent_end without the continuation flag closes the cycle, and OVRCR treats
// that Ready as Observed because the stop hook may still continue — a later agent_start
// then opens a new cycle. Session switches happen in place on this same instance, so a
// switch re-announces the conversation with session_start.
import { createReporter, idOf, modelId, outcomeOf } from "./ovrcr-reporting-transport.mjs";

// Replaced with the JSON-encoded absolute path of the supervising ovrcr binary when the
// receiver materializes this file for one invocation.
const OVRCR_BINARY = __OVRCR_BINARY__;

function currentModel(ctx) {
  return modelId(ctx.getModel?.());
}

export default function (omp) {
  if (!process.env.OVRCR_AGENT_SOCKET || !process.env.OVRCR_AGENT_TOKEN) return;
  const { producer, report, flush, reattach } = createReporter({
    binary: OVRCR_BINARY,
    helperArgs: ["report", "omp", "--stdin"],
  });

  // Cycles are numbered for the life of this instance, never per conversation: a switch
  // happens in place on this same instance, and the receiver rejects a repeated
  // `<instance>:<run>` identity as the continuation of a cycle it has already published.
  let issued = 0;

  // `previous` is the conversation this announcement says it left, as an id: an Oh My Pi
  // switch carries `previousSessionFile`, and the receiver pauses on a switch that names a
  // conversation OVRCR is not bound to. Startup carries none, which is no expectation.
  function announce(ctx, reason, previous) {
    producer.run = 0; // an announcement belongs to no cycle
    producer.open = false;
    producer.outcome = "none";
    const model = currentModel(ctx);
    return report("session_start", ctx, {
      reason,
      previous,
      ...(model ? { model } : {}),
    });
  }

  omp.on("session_start", (_event, ctx) => announce(ctx, "startup", null));
  omp.on("session_switch", (event, ctx) => announce(ctx, event.reason, idOf(event.previousSessionFile)));
  omp.on("agent_start", (_event, ctx) => {
    // A start inside an open cycle is a continuation: same run, outcome so far kept.
    if (!producer.open) {
      issued += 1;
      producer.run = issued;
      producer.open = true;
      producer.outcome = "none";
    }
    return report("agent_start", ctx);
  });
  omp.on("agent_end", (event, ctx) => {
    producer.outcome = outcomeOf(event.messages ?? []);
    const willContinue = Boolean(event.willContinue);
    const end = report("agent_end", ctx, { will_continue: willContinue });
    if (willContinue) return end;
    // Oh My Pi does not await this emit, so the next agent_start can arrive while a helper
    // is in flight: frame the settled event and close the cycle in this same tick. The
    // transport FIFO keeps the end before the settled on the wire.
    const settled = report("agent_settled", ctx);
    producer.open = false;
    return Promise.all([end, settled]).then(([, sent]) => sent);
  });
  // The model the session is using right now: Oh My Pi fires this on /model, cycling, and
  // any other set. Until the first report the sidebar stays the agent name alone; a later
  // switch replaces the previous model. Launch flags and terminal text are never read.
  omp.on("model_select", (event, ctx) => {
    const model = modelId(event.model);
    return model ? report("model_select", ctx, { model }) : Promise.resolve(false);
  });
  // Root-only: task and advisor sessions run in this same process and their approval
  // events carry their own session id (or ""), never the interactive root's.
  const root = (event, ctx) =>
    Boolean(event?.sessionId) && event.sessionId === ctx.sessionManager.getSessionId();
  const request = (ns, id) => `${ns}:${id}`;

  // Registering these two handlers disables Oh My Pi's speculative read execution
  // (its gate refuses speculation while any tool lifecycle handler is registered).
  omp.on("tool_approval_requested", (event, ctx) =>
    root(event, ctx)
      ? report("input_open", ctx, { namespace: "approval", request_id: request("approval", event.toolCallId), kind: "approval" })
      : Promise.resolve(false));
  omp.on("tool_approval_resolved", (event, ctx) =>
    root(event, ctx)
      ? report("input_close", ctx, { namespace: "approval", request_id: request("approval", event.toolCallId) })
      : Promise.resolve(false));
  // A question is the ask tool's lifetime, never dialog visibility: the request opens with
  // the tool, a moment before the dialog, and covers a question queued behind another
  // dialog. Tool events carry no session id, so the mode guard in report() is what keeps
  // in-process children silent. `args` and `result` are never read.
  omp.on("tool_execution_start", (event, ctx) =>
    event?.toolName === "ask"
      ? report("input_open", ctx, { namespace: "question", request_id: request("question", event.toolCallId), kind: "select" })
      : Promise.resolve(false));
  omp.on("tool_execution_end", (event, ctx) =>
    event?.toolName === "ask"
      ? report("input_close", ctx, { namespace: "question", request_id: request("question", event.toolCallId) })
      : Promise.resolve(false));

  // Tree navigation moves the conversation to a node this producer never reported: the
  // response cycle it was in the middle of is gone, and the only activity that survives is
  // what Oh My Pi's own API answers. Historical responses are never replayed as new ones.
  // Compaction is deliberately not subscribed: it preserves the conversation binding.
  omp.on("session_tree", (_event, ctx) => {
    producer.open = false;
    producer.outcome = "none";
    return report("cycle_invalidated", ctx, { idle: Boolean(ctx.isIdle?.()) });
  });

  omp.on("session_shutdown", (_event, ctx) => {
    const sent = flush("session_shutdown", ctx, { reason: "quit" });
    producer.disabled = true; // shutdown handlers run in parallel under a 2 s budget
    return sent;
  });

  // The explicit reattachment action: it runs inside the already-loaded extension, so it
  // never needs a fresh native launch to restore reporting for this same session — and it
  // never reloads the conversation, which #88 forbids as a reconnection substitute.
  omp.registerCommand("ovrcr-reattach", {
    description: "Reattach OVRCR reporting for this session",
    handler: async (_args, ctx) => {
      const model = currentModel(ctx);
      const ok = await reattach(ctx, {
        idle: Boolean(ctx.isIdle?.()),
        ...(model ? { model } : {}),
      });
      ctx.ui?.notify?.(ok ? "OVRCR reporting reattached" : "OVRCR reporting is unavailable", ok ? "info" : "warning");
    },
  });
}

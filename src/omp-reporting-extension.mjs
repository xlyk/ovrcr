// OVRCR reporting for a managed interactive Oh My Pi session. Loaded only by
// `ovrcr agent run omp` through an explicit -e path beside the user's own extensions;
// inert without the private invocation channel and outside the terminal UI (task and
// advisor sessions run in this same process with mode "print"). Oh My Pi has no settled
// event: an agent_end without the continuation flag closes the cycle, and OVRCR treats
// that Ready as Observed because the stop hook may still continue — a later agent_start
// then opens a new cycle. Session switches happen in place on this same instance, so a
// switch re-announces the conversation with session_start.
import { createReporter, outcomeOf } from "./ovrcr-reporting-transport.mjs";

// Replaced with the JSON-encoded absolute path of the supervising ovrcr binary when the
// receiver materializes this file for one invocation.
const OVRCR_BINARY = __OVRCR_BINARY__;

export default function (omp) {
  if (!process.env.OVRCR_AGENT_SOCKET || !process.env.OVRCR_AGENT_TOKEN) return;
  const { producer, report } = createReporter({
    binary: OVRCR_BINARY,
    helperArgs: ["report", "omp", "--stdin"],
  });

  // Cycles are numbered for the life of this instance, never per conversation: a switch
  // happens in place on this same instance, and the receiver rejects a repeated
  // `<instance>:<run>` identity as the continuation of a cycle it has already published.
  let issued = 0;

  function announce(ctx, reason) {
    producer.run = 0; // an announcement belongs to no cycle
    producer.open = false;
    producer.outcome = "none";
    return report("session_start", ctx, { reason });
  }

  omp.on("session_start", (_event, ctx) => announce(ctx, "startup"));
  omp.on("session_switch", (event, ctx) => announce(ctx, event.reason));
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
  omp.on("agent_end", async (event, ctx) => {
    producer.outcome = outcomeOf(event.messages ?? []);
    const willContinue = Boolean(event.willContinue);
    const end = report("agent_end", ctx, { will_continue: willContinue });
    if (willContinue) return end;
    await end;
    const settled = report("agent_settled", ctx);
    producer.open = false;
    return settled;
  });
  omp.on("session_shutdown", (_event, ctx) => {
    const sent = report("session_shutdown", ctx, { reason: "quit" });
    producer.disabled = true; // shutdown handlers run in parallel under a 2 s budget
    return sent;
  });
}

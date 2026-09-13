// OVRCR reporting for a managed interactive Pi session. Loaded only by
// `ovrcr agent run pi` through an explicit -e path beside the user's own extensions;
// inert without the private invocation channel. Sends bounded identifiers and
// discriminants to a directly spawned helper, never prompts, responses or tool data.
// One response cycle spans every continuation Pi runs — retry, automatic compaction, a
// queued follow-up — because Pi emits `agent_start` at the top of both its agent loop and
// its continue loop, and a single `agent_settled` only once the whole prompt is finished.
// This extension owns that span: `run` advances on the first `agent_start` of a cycle and
// holds until settled, so a continuation reports work without opening a new cycle.
// Bounded delivery lives in the transport module materialized beside this file.
import { createReporter, outcomeOf } from "./ovrcr-reporting-transport.mjs";

// Replaced with the JSON-encoded absolute path of the supervising ovrcr binary when the
// receiver materializes this file for one invocation.
const OVRCR_BINARY = __OVRCR_BINARY__;

export default function (pi) {
  if (!process.env.OVRCR_AGENT_SOCKET || !process.env.OVRCR_AGENT_TOKEN) return;
  const { producer, report } = createReporter({
    binary: OVRCR_BINARY,
    helperArgs: ["report", "pi", "--stdin"],
  });

  pi.on("session_start", (event, ctx) => {
    producer.run = 0;
    producer.open = false;
    producer.outcome = "none";
    return report("session_start", ctx, { reason: event.reason });
  });
  pi.on("agent_start", (_event, ctx) => {
    // A second start inside an open cycle is a continuation: same run, outcome so far kept.
    if (!producer.open) {
      producer.run += 1;
      producer.open = true;
      producer.outcome = "none";
    }
    return report("agent_start", ctx);
  });
  pi.on("agent_end", (event, ctx) => {
    producer.outcome = outcomeOf(event.messages ?? []);
    return report("agent_end", ctx);
  });
  pi.on("agent_settled", (_event, ctx) => {
    const sent = report("agent_settled", ctx);
    producer.open = false; // the cycle closes here, whatever continuations it contained
    return sent;
  });
  // Pi coalesces nested or overlapping prompts into one outer span and does not await these
  // handlers; one counter per instance is therefore a complete identity. The prompt title is
  // never read.
  pi.on("ui_prompt_start", (event, ctx) => {
    producer.prompt += 1;
    return report("input_open", ctx, { request_id: `${producer.instance}:p${producer.prompt}`, kind: event.kind });
  });
  pi.on("ui_prompt_end", (_event, ctx) =>
    report("input_close", ctx, { request_id: `${producer.instance}:p${producer.prompt}` }));
  pi.on("session_shutdown", (event, ctx) => {
    const sent = report("session_shutdown", ctx, { reason: event.reason });
    producer.disabled = true; // Pi re-runs factories after replacement or reload
    return sent;
  });
}

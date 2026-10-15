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
import { spawn } from "node:child_process";
import { randomUUID } from "node:crypto";
import { mkdir, open, lstat, rename, link, unlink } from "node:fs/promises";
import { constants } from "node:fs";
import { join } from "node:path";
import { createDelegationObserver } from "./ovrcr-delegation-observer.mjs";
import { createReporter, idOf, modelId, outcomeOf } from "./ovrcr-reporting-transport.mjs";

// Replaced with the JSON-encoded absolute path of the supervising ovrcr binary when the
// receiver materializes this file for one invocation.
const OVRCR_BINARY = __OVRCR_BINARY__;

function currentModel(ctx) {
  // ExtensionContext exposes the live model as `ctx.model` (a getter over the session).
  return modelId(ctx.model);
}

export default function (pi) {
  if (!process.env.OVRCR_AGENT_SOCKET || !process.env.OVRCR_AGENT_TOKEN) return;
  const { producer, report, flush, reattach } = createReporter({
    binary: OVRCR_BINARY,
    helperArgs: ["report", "pi", "--stdin"],
  });

  registerDelegation(pi);

  pi.on("session_start", (event, ctx) => {
    producer.run = 0;
    producer.open = false;
    producer.outcome = "none";
    const model = currentModel(ctx);
    return report("session_start", ctx, {
      reason: event.reason,
      previous: idOf(event.previousSessionFile),
      ...(model ? { model } : {}),
    });
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
  // The model the session is using right now: Pi fires this on /model, cycling, and any
  // other set. Until the first report the sidebar stays the agent name alone; a later
  // switch replaces the previous model. Launch flags and terminal text are never read.
  pi.on("model_select", (event, ctx) => {
    const model = modelId(event.model);
    return model ? report("model_select", ctx, { model }) : Promise.resolve(false);
  });
  // Pi coalesces nested or overlapping prompts into one outer span and does not await these
  // handlers; one counter per instance is therefore a complete identity. The prompt title is
  // never read.
  pi.on("ui_prompt_start", (event, ctx) => {
    producer.prompt += 1;
    return report("input_open", ctx, { namespace: "prompt", request_id: `${producer.instance}:p${producer.prompt}`, kind: event.kind });
  });
  pi.on("ui_prompt_end", (_event, ctx) =>
    report("input_close", ctx, { namespace: "prompt", request_id: `${producer.instance}:p${producer.prompt}` }));
  // Tree navigation moves the conversation to a node this producer never reported: the
  // response cycle it was in the middle of is gone, and the only activity that survives is
  // what Pi's own API answers. Historical responses are never replayed as new ones.
  pi.on("session_tree", (_event, ctx) => {
    producer.open = false;
    producer.outcome = "none";
    return report("cycle_invalidated", ctx, { idle: Boolean(ctx.isIdle?.()) });
  });
  pi.on("session_shutdown", (event, ctx) => {
    const sent = flush("session_shutdown", ctx, { reason: event.reason });
    producer.disabled = true; // Pi re-runs factories after replacement or reload
    return sent;
  });
  // The explicit reattachment action: it runs inside the already-loaded extension, so it
  // never needs a fresh native launch to restore reporting for this same session.
  pi.registerCommand("ovrcr-reattach", {
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


// This control client is separate from reporting: task contents never enter Reporter.
function registerDelegation(pi) {
  const workflow = randomUUID();
  const controller = randomUUID();
  let active = true;
  pi.on("session_shutdown", () => { active = false; observer.stop(); });
  const auth = () => {
    if (!active) throw new Error("Pi session changed; reconcile the assignment from its owner");
    const session = Number(process.env.OVRCR_DELEGATION_SESSION);
    const capability = process.env.OVRCR_DELEGATION_TOKEN;
    if (!Number.isSafeInteger(session) || session <= 0 || !capability || !process.env.OVRCR_DELEGATION_SOCKET) throw new Error("Delegation requires a managed Pi session");
    return { session, capability };
  };
  const call = (request, { signal } = {}) => new Promise((resolve, reject) => {
    const child = spawn(OVRCR_BINARY, ["--json", "delegation"], {
      env: { ...process.env, OVRCR_SOCKET: process.env.OVRCR_DELEGATION_SOCKET, OVRCR_HOME: process.env.OVRCR_DELEGATION_HOME },
      stdio: ["pipe", "pipe", "pipe"],
    });
    let output = "";
    let failed = false;
    let stderr = "";
    const timer = setTimeout(() => { failed = true; child.kill(); reject(new Error("control outcome unknown; inspect the original handle before any further action")); }, 30000);
    const abort = () => {
      failed = true; child.kill(); clearTimeout(timer);
      const error = new Error("observation wait cancelled; child work continues");
      error.name = "AbortError";
      reject(error);
    };
    signal?.addEventListener("abort", abort, { once: true });
    if (signal?.aborted) abort();
    child.stdout.on("data", bytes => {
      output += bytes;
      if (output.length > 1024 * 1024) { failed = true; child.kill(); clearTimeout(timer); reject(new Error("control response exceeded bound")); }
    });
    child.stderr.on("data", bytes => { if (stderr.length < 65536) stderr += bytes; });
    child.on("error", error => { clearTimeout(timer); reject(error); });
    child.on("close", code => {
      clearTimeout(timer);
      signal?.removeEventListener("abort", abort);
      if (failed) return;
      try {
        if (code !== 0) {
          let failure;
          try { failure = JSON.parse(stderr).error; } catch {}
          const error = new Error(failure?.message ?? "control outcome unknown");
          error.code = failure?.code;
          error.refused = failure && failure.code !== "Internal";
          throw error;
        }
        const response = JSON.parse(output);
        resolve(response);
      } catch (error) { reject(error); }
    });
    child.stdin.on("error", () => {});
    child.stdin.end(JSON.stringify(request));
  });
  const observer = createDelegationObserver({ pi, call, auth });
  pi.registerCommand("ovrcr-observe-reconnect", { description: "Reconnect assignment observations without changing child work", handler: () => observer.reconnect() });
  const object = (properties, required = Object.keys(properties)) => ({ type: "object", properties, required, additionalProperties: false });
  const string = { type: "string" };
  const originalIntent = async (handle, prefix = "") => {
    if (!/^[A-Za-z0-9_-]{1,128}$/.test(handle)) throw new Error("invalid original request identity");
    const directory = join(process.env.OVRCR_DELEGATION_HOME ?? "", "parent-intents");
    const parent = await lstat(directory);
    if (!parent.isDirectory() || parent.isSymbolicLink() || (parent.mode & 0o077) || parent.uid !== process.getuid()) throw new Error("original intent directory is not private");
    const file = await open(join(directory, `${prefix}${handle}.json`), constants.O_RDONLY | constants.O_NOFOLLOW | constants.O_NONBLOCK);
    try {
      const stat = await file.stat();
      if (!stat.isFile() || stat.size > 262144 || (stat.mode & 0o077) || stat.uid !== process.getuid()) throw new Error("invalid original intent file");
      const buffer = Buffer.alloc(262145);
      let length = 0;
      while (length < buffer.length) {
        const { bytesRead } = await file.read(buffer, length, buffer.length - length, null);
        if (!bytesRead) break;
        length += bytesRead;
      }
      const bytes = buffer.subarray(0, length);
      const after = await lstat(directory);
      if (bytes.length > 262144 || after.ino !== parent.ino || after.dev !== parent.dev || after.isSymbolicLink()) throw new Error("original intent directory changed");
      const contract = JSON.parse(bytes);
      if (contract.request !== handle || contract.schema !== 1) throw new Error("original intent identity changed");
      return contract;
    } finally { await file.close(); }
  };
  const tool = (name, description, parameters, run) => pi.registerTool({ name, label: name, description, parameters, async execute(_id, args, signal, _onUpdate, ctx) {
    if (!args || typeof args !== "object") throw new Error("tool arguments must be an object");
    for (const key of parameters.required ?? []) if (!(key in args)) throw new Error(`missing required argument: ${key}`);
    const response = await run(args, ctx, signal);
    if (name !== "ovrcr_publish" && response?.identity?.session) observer.watch(response.handle);
    observer.acknowledge();
    return { content: [{ type: "text", text: JSON.stringify(response) }], details: response };
  }});
  tool("ovrcr_start", "Start one read-only Pi research assignment in an existing workspace. Returns a stable handle without waiting for completion. Use handle to finish an initial delivery still waiting for Pi startup; unknown sends are never replayed.", { ...object({ handle: string, project: string, workspace: string, scope: string, provider: string, model_provider: string, model: string, permitted_actions: { type: "array", items: string }, acceptance_evidence: { type: "array", items: string }, mandatory_capabilities: { type: "array", items: string } }, []), anyOf: [{ required: ["handle"] }, { required: ["project", "workspace", "scope", "model_provider", "model", "acceptance_evidence"] }] }, async args => {
    let handle = args.handle;
    const required = ["project", "workspace", "scope", "model_provider", "model"];
    if (handle !== undefined ? (typeof handle !== "string" || !handle || Object.keys(args).some(key => key !== "handle")) : required.some(key => typeof args[key] !== "string" || !args[key].trim()) || !Array.isArray(args.acceptance_evidence) || !args.acceptance_evidence.length) {
      return { handle: handle ?? null, dispatch: "Refused", reason: "Provide only handle to continue, or project, workspace, scope, model_provider, model and acceptance_evidence to start" };
    }
    let attempted = false;
    try {
      let snapshot;
      let contract;
      if (handle) {
        contract = await originalIntent(handle);
        attempted = true;
        try { snapshot = await call({ Reconcile: { auth: auth(), request: handle } }); }
        catch (error) {
          if (error.code !== "NotFound") throw error;
        }
        // Verify the exact saved contract even when its Server record already exists.
        // Prepare only repairs its authenticated immutable publication, never a launch/input.
        snapshot = await call({ Prepare: { auth: auth(), contract } });
      }
      else {
        handle = randomUUID();
        contract = { schema: 1, workflow, controller, task: randomUUID(), request: handle, project: args.project, workspace: args.workspace, scope: args.scope, provider: args.provider ?? "pi", model_provider: args.model_provider, model: args.model, permitted_actions: args.permitted_actions ?? ["read"], acceptance_evidence: args.acceptance_evidence ?? [], mandatory_capabilities: args.mandatory_capabilities ?? ["read", "result_file"] };
        if (JSON.stringify(contract).length > 262144 || contract.scope.length > 32768 || contract.acceptance_evidence.length > 32 || contract.acceptance_evidence.some(value => typeof value !== "string" || !value || value.length > 4096)) {
          return { handle, dispatch: "Refused", reason: "contract exceeds supported content limits" };
        }
        auth();
        const root = process.env.OVRCR_DELEGATION_HOME;
        if (!root) throw new Error("managed instance directory unavailable");
        const directory = join(root, "parent-intents");
        await mkdir(directory, { mode: 0o700, recursive: true });
        const stat = await lstat(directory);
        if (!stat.isDirectory() || stat.isSymbolicLink() || (stat.mode & 0o077)) throw new Error("intent directory must be private");
        const temporary = join(directory, `.${handle}.tmp`);
        const file = await open(temporary, "wx", 0o600);
        try { await file.writeFile(JSON.stringify(contract)); await file.sync(); } finally { await file.close(); }
        await rename(temporary, join(directory, `${handle}.json`));
        const directoryHandle = await open(directory, "r");
        try { await directoryHandle.sync(); } finally { await directoryHandle.close(); }
        attempted = true;
        snapshot = await call({ Prepare: { auth: auth(), contract } });
      }
      const receipt = operation => snapshot.receipts?.find(value => value.operation === operation);
      if (receipt("Prepare")?.outcome !== "Applied") snapshot = await call({ Prepare: { auth: auth(), contract } });
      if (receipt("Prepare")?.outcome !== "Applied") return snapshot;
      if (snapshot.dispatch === "Prepared" && (!receipt("Launch") || receipt("Launch").outcome === "Accepted")) {
        snapshot = await call({ Launch: { auth: auth(), handle, identity: receipt("Launch")?.identity ?? snapshot.identity } });
      }
      // Launched is a known no-input state. Polling it does not replay a send.
      for (let attempt = 0; snapshot.dispatch === "Launched" && (!receipt("InitialInput") || receipt("InitialInput").outcome === "Accepted") && attempt < 100; attempt++) {
        snapshot = await call({ SendInitial: { auth: auth(), handle, identity: snapshot.identity } });
        if (snapshot.dispatch === "Launched") await new Promise(resolve => setTimeout(resolve, 100));
      }
      return snapshot;
    } catch (error) { return { handle, dispatch: error.refused || !attempted ? "Refused" : "Unknown", transport: attempted ? "Unknown" : "NotSent", reason: String(error.message), recovery: "Call ovrcr_start with only this original handle; do not start a replacement assignment" }; }
  });
  // Followups retain their original private request even if the first reply is lost.
  const saveFollowup = async intent => {
    const root = process.env.OVRCR_DELEGATION_HOME;
    if (!root) throw new Error("managed instance directory unavailable");
    const directory = join(root, "parent-intents");
    await mkdir(directory, { mode: 0o700, recursive: true });
    const stat = await lstat(directory);
    if (!stat.isDirectory() || stat.isSymbolicLink() || (stat.mode & 0o077) || stat.uid !== process.getuid()) throw new Error("intent directory must be private");
    const bytes = JSON.stringify(intent);
    if (Buffer.byteLength(bytes) > 262144) throw new Error("followup intent exceeds supported content limits");
    const temporary = join(directory, `.${intent.request}.tmp`);
    const file = await open(temporary, "wx", 0o600);
    try { await file.writeFile(bytes); await file.sync(); } finally { await file.close(); }
    try { await link(temporary, join(directory, `followup-${intent.request}.json`)); }
    finally { await unlink(temporary); }
    const held = await open(directory, "r");
    try { await held.sync(); } finally { await held.close(); }
  };
  tool("ovrcr_followup", "Send a scoped followup to this assignment's same safe child. Supply expected_identity, expected_result_version and text, plus revised_contract only for explicit changed requirements. Each call expects a new result version. Recover a lost reply with only handle and its original request; unknown/partial input must never be resent under a new request.", { ...object({ handle: string, request: string, expected_identity: { type: "object" }, expected_result_version: { type: "integer", minimum: 1 }, text: string, revised_contract: { type: ["object", "null"] } }, ["handle"]), anyOf: [{ required: ["handle", "request"] }, { required: ["handle", "expected_identity", "expected_result_version", "text"] }] }, async args => {
    let request = args.request;
    let attempted = false;
    try {
      if (typeof args.handle !== "string" || !/^[A-Za-z0-9_-]{1,128}$/.test(args.handle)) throw new Error("invalid assignment handle");
      let intent;
      if (request !== undefined) {
        if (Object.keys(args).some(key => !["handle", "request"].includes(key))) throw new Error("recovery requires only handle and original request");
        intent = await originalIntent(request, "followup-");
        if (intent.handle !== args.handle) throw new Error("original followup belongs to another assignment");
      } else {
        if (!Number.isSafeInteger(args.expected_result_version) || args.expected_result_version < 1 || !args.expected_identity || typeof args.expected_identity !== "object" || Array.isArray(args.expected_identity) || typeof args.text !== "string" || !args.text.trim() || Buffer.byteLength(args.text) > 32768 || Object.keys(args).some(key => !["handle", "expected_identity", "expected_result_version", "text", "revised_contract"].includes(key))) throw new Error("provide expected_identity, expected_result_version and 1..32768 bytes of scoped text");
        request = randomUUID();
        intent = { schema: 1, handle: args.handle, request, expected_identity: args.expected_identity, expected_result_version: args.expected_result_version, text: args.text, revised_contract: args.revised_contract ?? null };
        auth();
        await saveFollowup(intent);
      }
      attempted = true;
      const { schema: _schema, ...fields } = intent;
      const snapshot = await call({ Followup: { auth: auth(), ...fields } });
      return { ...snapshot, request };
    } catch (error) { return { handle: args.handle, request: request ?? null, dispatch: error.refused || !attempted ? "Refused" : "Unknown", transport: attempted ? "Unknown" : "NotSent", reason: String(error.message), recovery: "Recover with only this handle and original request; do not create another request while input is unknown or partial" }; }
  });
  tool("ovrcr_accept", "Record the parent's explicit acceptance of the exact current result identity, version and digest. This records your judgment; collection, Ready and child success never accept automatically.", object({ handle: string, identity: { type: "object" }, result_version: { type: "integer", minimum: 1 }, result_digest: string }), async args => {
    try { return await call({ RecordAcceptance: { auth: auth(), ...args } }); }
    catch (error) { return { handle: args.handle, dispatch: error.refused ? "Refused" : "Unknown", reason: String(error.message), recovery: "Retry acceptance only with the same exact current result reference" }; }
  });
  tool("ovrcr_inspect", "Inspect an owned assignment without changing it. Activity is not acceptance.", object({ handle: string }), args => call({ Inspect: { auth: auth(), handle: args.handle } }));
  tool("ovrcr_collect", "Collect the current validated submission without accepting it. Optional version explicitly reads historical evidence; historical_submission cannot satisfy current collection or acceptance.", object({ handle: string, version: { type: "integer", minimum: 1 } }, ["handle"]), args => call(args.version === undefined ? { Collect: { auth: auth(), handle: args.handle } } : { CollectVersion: { auth: auth(), handle: args.handle, version: args.version } }));
  tool("ovrcr_wait", "Wait for bounded content-free assignment observations. Cancellation leaves child work running. Fetch available files with ovrcr_collect; use exact publication identity to mark handling separately from acceptance.", object({ handles: { type: "array", items: string, minItems: 1, maxItems: 64 }, after: { type: "object" }, wait_ms: { type: "integer", minimum: 0, maximum: 20000 } }, []), (args, _ctx, signal) => observer.wait(args, signal));
  tool("ovrcr_mark_handled", "Acknowledge handling of this exact immutable publication. Does not accept task success.", object({ handle: string, publication: { type: "object" } }), args => call({ MarkHandled: { auth: auth(), handle: args.handle, publication: args.publication } }));
  tool("ovrcr_publish", "Publish the assigned child's immutable research result. Use the exact identity and result_version from the current brief (result_version is submission.version). This does not accept the work.", object({ handle: string, submission: { type: "object" } }), args => call({ Publish: { auth: auth(), handle: args.handle, submission: args.submission } }));
}

import { applyReviewFixture } from "./pi_review.mjs";
import { isDeepStrictEqual } from "node:util";
import { createHash } from "node:crypto";
import { dependencyResult } from "./pi_dependencies.mjs";
// Test host for the OVRCR reporting extensions. It serves both harnesses: Pi and Oh My
// Pi share the event vocabulary, and the grammar below covers the events only one of them
// emits (agent_settled for Pi, session_switch and a continuing agent_end for Oh My Pi).
// As a library it fires the real registered handlers with a fake ExtensionContext
// (node --test). Run directly it is a fake `pi` or `omp` executable for the Rust lifecycle
// tests: it loads the `-e` extension and turns stdin lines into extension events, printing
// PI_CALLBACK=<n> after each one.
import { pathToFileURL } from "node:url";
import { createInterface } from "node:readline";
import { execFileSync, spawnSync } from "node:child_process";
import { writeFileSync, readFileSync, existsSync, appendFileSync, renameSync, unlinkSync } from "node:fs";
import { join, dirname } from "node:path";
import { registerHooks } from "node:module";
import { PassThrough } from "node:stream";

// Supported Node loader hook; optional fixture API is explicitly enabled only
// for isolated process tests. Production still uses Pi's installed loader.
if (process.env.OVRCR_TEST_PI_INTERRUPT_BINDING) registerHooks({
  resolve(specifier, context, nextResolve) {
    if (specifier === "@earendil-works/pi-tui") return { url: new URL("./pi_keybindings.mjs", import.meta.url).href, shortCircuit: true };
    return nextResolve(specifier, context);
  },
});

export function assistant(stopReason) {
  return { role: "assistant", stopReason, content: [] };
}

export async function createHost(extensionPath, { mode = "tui", session = "session-a", idle = true, model = undefined } = {}) {
  const handlers = new Map();
  const commands = new Map();
  const tools = new Map();
  let state;
  const api = {
    on(event, handler) {
      if (!handlers.has(event)) handlers.set(event, []);
      handlers.get(event).push(handler);
    },
    registerTool(tool) { tools.set(tool.name, tool); },
    appendEntry(customType, data) {
      const entry = { type: "custom", customType, data };
      state.entries.push(entry);
      const file = ctx.sessionManager.getSessionFile();
      if (file) appendFileSync(file, JSON.stringify(entry) + "\n");
    },
    sendMessage(message, options) {
      const delivery = state.idle ? "idle" : "followUp";
      const wake = { message, options, delivery };
      state.messages.push(wake);
      if (!state.idle) state.followUps.push(wake);
      if (process.env.OVRCR_TEST_PI_WAKE_LOG) appendFileSync(process.env.OVRCR_TEST_PI_WAKE_LOG, JSON.stringify({ pid: process.pid, session: state.session, ...wake }) + "\n");
    },
    registerCommand(name, options) {
      commands.set(name, options);
    },
  };
  const module = await import(pathToFileURL(extensionPath).href);
  await module.default(api);
  state = {
    mode,
    session,
    idle,
    notices: [],
    messages: [],
    followUps: [],
    entries: [],
    model: model === undefined ? undefined : { id: model, provider: "test" },
  };
  const ctx = {
    get mode() { return state.mode; },
    hasUI: true,
    sessionManager: {
      getSessionId: () => state.session,
      getBranch: () => {
        const file = ctx.sessionManager.getSessionFile();
        return file && existsSync(file) ? readFileSync(file, "utf8").trim().split("\n").map(line => JSON.parse(line)).filter(entry => entry.type === "custom") : state.entries;
      },
      getSessionFile: () => {
        if (state.ephemeral) return undefined;
        if (state.history) return state.history;
        if (!process.env.OVRCR_TEST_HISTORY) return undefined;
        const file = `${process.env.OVRCR_TEST_HISTORY}/${state.session}.jsonl`;
        if (!existsSync(file)) writeFileSync(file, JSON.stringify({ type: "session", version: 3, id: state.session, cwd: process.cwd() }) + "\n");
        return file;
      },
    },
    isIdle: () => state.idle,
    get model() { return state.model; },
    ui: { notify: (message, level) => state.notices.push([message, level]) },
  };
  return {
    state,
    registered: () => [...handlers.keys()].sort(),
    commands: () => [...commands.keys()].sort(),
    tools: () => [...tools.keys()].sort(),
    toolDefinition: name => tools.get(name),
    tool: (name, args, signal) => tools.get(name).execute("host-tool", args, signal, undefined, ctx),
    run: (name, args = []) => commands.get(name).handler(args, ctx),
    async emit(event) {
      const results = [];
      for (const handler of handlers.get(event.type) ?? []) results.push(await handler(event, ctx));
      return results;
    },
  };
}

// stdin grammar: session_start[:<id>[:<reason>]] | agent_start
//   | session_replace[:<id>[:<reason>]] | session_tree | session_compact
//   | run_command:<name> | idle:<true|false>
//   | agent_end:<ok|error|aborted|none>[:continue] | agent_settled
//   | model:<id> | model_select:<id>
//   | ui_prompt_start:<kind> | ui_prompt_end:<kind>
//   | tool_approval_requested:<id> | tool_approval_resolved:<id>:<true|false>
//   | foreign_approval:<id> | tool_execution_start:<toolName>:<id>
//   | tool_execution_end:<toolName>:<id>[:error]
//   | session_switch[:<id>[:<reason>[:<previous id>]]] | plugin_refresh
//   | session_shutdown:<reason>
//   | mode:<tui|rpc|print> | exit
async function main() {
  // The acceptance proxy changes only control transport; reporting stays direct.
  if (process.env.OVRCR_TEST_DELEGATION_CONTROL_SOCKET) process.env.OVRCR_DELEGATION_SOCKET = process.env.OVRCR_TEST_DELEGATION_CONTROL_SOCKET;
  const args = process.argv.slice(2);
  const at = args.indexOf("-e");
  const extension = at >= 0 ? args[at + 1] : null;
  let host = extension ? await createHost(extension) : null;
  let retiredHost = null;
  if (host && process.env.OVRCR_TEST_DELEGATION) {
    const resumeArg = args.find(arg => arg.startsWith("--session="));
    const resumeAt = args.indexOf("--session");
    const resume = resumeArg?.slice("--session=".length) ?? (resumeAt >= 0 ? args[resumeAt + 1] : undefined);
    host.state.session = resume ? JSON.parse(readFileSync(resume, "utf8").split("\n")[0]).id : `fixture-${process.pid}`;
    if (resume) host.state.history = resume;
    host.state.model = { id: args[args.indexOf("--model") + 1] || "fixture-model", provider: "test" };
    if (args.includes("--tools") && process.env.OVRCR_TEST_DELEGATION_INITIAL_MODEL) host.state.model.id = process.env.OVRCR_TEST_DELEGATION_INITIAL_MODEL;
    const modelGate = args.includes("--tools") && process.env.OVRCR_TEST_DELEGATION_MODEL_GATE;
    const pendingModel = modelGate && !existsSync(modelGate) ? host.state.model : null;
    if (pendingModel) host.state.model = undefined;
    await host.emit({ type: "session_start", reason: "startup" });
    if (pendingModel) void (async () => {
      while (!existsSync(modelGate)) await new Promise(resolve => setTimeout(resolve, 10));
      // Model readiness changes within this exact binding, never a rebind/reload.
      host.state.model = pendingModel;
      await host.emit({ type: "model_select", model: pendingModel });
      writeFileSync(`${modelGate}.reported`, "same-binding-model-reported");
    })();
    if (args.includes("--tools") && process.env.OVRCR_TEST_DELEGATION_REBIND_GATE) {
      const gate = process.env.OVRCR_TEST_DELEGATION_REBIND_GATE;
      const original = host.state.session;
      void (async () => {
        while (!existsSync(gate)) await new Promise(resolve => setTimeout(resolve, 5));
        host.state.model = {id:"fixture-model",provider:"test"};
        for (const session of ["replacement-B",original]) {
          host.state.session = session;
          await host.emit({type:"session_start",reason:"new"});
        }
        writeFileSync(`${gate}.done`,"rebound");
      })();
    }
  }
  if (process.env.OVRCR_TEST_PROBE) {
    writeFileSync(
      process.env.OVRCR_TEST_PROBE,
      `${process.env.OVRCR_AGENT_SOCKET ?? ""}\n${process.env.OVRCR_AGENT_TOKEN ?? ""}\n${process.pid}\n`,
    );
  }
  let index = 0;
  const pendingTools = new Map();
  let toolPublication = 0;
  let previousSubmission = null;
  let activeBrief;
  let interrupting = false;
  let pendingQuestion = null;
  const toolOutput = (path, value) => {
    const temporary = `${path}.${process.pid}.${toolPublication++}.tmp`;
    writeFileSync(temporary, JSON.stringify(value));
    renameSync(temporary, path);
  };
  // Start flowing stdin only after startup awaits, immediately before readline.
  // Otherwise an observed binding can prompt input before readline owns it.
  let input = process.stdin;
  if (process.env.OVRCR_TEST_DELEGATION_BYTES) {
    input = new PassThrough();
    process.stdin.on("end", () => input.end());
    const session = process.env.OVRCR_DELEGATION_SESSION;
    if (!/^[1-9][0-9]*$/.test(session ?? "")) throw new Error("byte counter requires assigned session identity");
    process.stdin.setRawMode?.(true);
    process.stdin.on("data", bytes => {
      appendFileSync(join(process.env.OVRCR_TEST_DELEGATION_BYTES, session), `${bytes.length}\n`);
      if (bytes.length === 1 && bytes[0] === 0x1b && activeBrief && !interrupting) {
        interrupting = true;
        const { path, brief } = activeBrief;
        void (async () => {
          await host.emit({ type: "agent_end", messages: [assistant("aborted")] });
          writeFileSync(`${path}.interrupt-consumed`, "CHILD_NATIVE_ESCAPE_CONSUMED");
          process.stdout.write("CHILD_NATIVE_ESCAPE_CONSUMED\n");
          if (brief.contract.scope.includes("INTERRUPT_SETTLEMENT_GATE")) {
            while (!existsSync(`${path}.interrupt-settle`)) await new Promise(resolve => setTimeout(resolve, 10));
          }
          await host.emit({ type: "agent_settled" });
          writeFileSync(`${path}.interrupt-settled`, "CHILD_NATIVE_SETTLED");
          process.stdout.write("CHILD_NATIVE_SETTLED\n");
        })();
        return; // The fixture native handler consumed this byte before its editor.
      }
      if (bytes.length === 1 && bytes[0] === 0x1b && activeBrief) return;
      input.write(bytes);
    });
  }
  const lines = createInterface({ input });
  process.stdout.write("PI_NATIVE_READY\n");
  for await (const line of lines) {
    const clean = line.replaceAll("\x1b[200~", "").replaceAll("\x1b[201~", "");
    const [command, a, b, c] = clean.split(":");
    if (host && command === "tool") {
      const request = JSON.parse(clean.slice(5));
      const controller = new AbortController();
      const run = async () => {
        try { toolOutput(request.output, await host.tool(request.name, request.args, controller.signal)); }
        catch (error) { toolOutput(request.output, { error: String(error) }); }
        finally { pendingTools.delete(request.output); }
      };
      if (request.background) { pendingTools.set(request.output, controller); void run(); }
      else await run();
      continue;
    }
    if (host && pendingQuestion && clean.startsWith("Read the scoped answer in ")) {
      const match = clean.match(/in (.+\/answer-[a-zA-Z0-9_-]+\.json) for question ([a-zA-Z0-9_-]+) version ([1-9][0-9]*)\./);
      if (!match) throw new Error("invalid scoped answer instruction");
      const answer = JSON.parse(readFileSync(match[1], "utf8"));
      const current = pendingQuestion.reference;
      if (!current || !isDeepStrictEqual(answer.question, current) || answer.question.question_id !== match[2] || answer.question.question_version !== Number(match[3])) throw new Error("answer is not for the current exact child question");
      appendFileSync(`${pendingQuestion.path}.answer-count`, "1\n");
      toolOutput(`${pendingQuestion.path}.answer-received`, answer);
      process.stdout.write("CHILD_SCOPED_ANSWER_RECEIVED\n");
      continue;
    }
    if (host && process.env.OVRCR_TEST_DELEGATION && (clean.startsWith("Perform the read-only research assignment in ") || clean.startsWith("Perform the coding assignment in "))) {
      const path = clean.match(/in (.+\/brief-v[1-9][0-9]*\.json)\. Read/)[1];
      const brief = JSON.parse(readFileSync(path, "utf8"));
      activeBrief = { path, brief };
      writeFileSync(`${path}.native-argv`, JSON.stringify(process.argv.slice(2)));
      interrupting = false;
      writeFileSync(`${path}.started`, "CHILD_RESEARCH_STARTED");
      appendFileSync(`${path}.input-count`, "1\n");
      process.stdout.write("CHILD_RESEARCH_STARTED\n");
      await host.emit({ type: "agent_start" });
      if (brief.contract.scope.includes("READY_WAKE")) {
        await host.emit({ type: "agent_end", messages: [assistant("stop")] });
        await host.emit({ type: "agent_settled" });
      }
      if (brief.contract.scope.includes("INPUT_WAKE")) await host.emit({ type: "ui_prompt_start", kind: "input" });
      if (brief.contract.scope.includes("FALLBACK_")) {
        const directory = dirname(path);
        const hash = bytes => createHash("sha256").update(bytes).digest("hex");
        // Child correlation comes entirely from exact immutable brief/contract
        // bytes. The production capture, never this fixture, owns attribution.
        const artifact = { schema: 1, context: {
          identity: brief.identity, result_version: brief.result_version,
          contract_digest: hash(readFileSync(join(directory, `contract-v${brief.identity.contract}.json`))),
          brief_digest: hash(readFileSync(path)), freshness_nonce: brief.fallback_nonce,
        }, deliverable: readFileSync("README", "utf8"), unresolved: [] };
        const bytes = Buffer.from(JSON.stringify(artifact));
        writeFileSync(join(directory, "fallback-evidence.json"), bytes, { mode: 0o600 });
        if (brief.contract.scope.includes("COMPLETE_INVALID")) writeFileSync(brief.output, "{incomplete original child publication", { mode: 0o600 });
        let marker = hash(bytes);
        if (brief.contract.scope.includes("OLD_SAME_RUN") || brief.contract.scope.includes("PREPARED_FUTURE")) {
          const stale = structuredClone(artifact);
          stale.context.result_version += brief.contract.scope.includes("PREPARED_FUTURE") ? 1 : -1;
          stale.context.freshness_nonce = hash("not the current immutable brief nonce");
          const other = Buffer.from(JSON.stringify(stale));
          writeFileSync(join(directory, "fallback-other-version.json"), other, { mode: 0o600 });
          marker = hash(other);
        }
        if (brief.contract.scope.includes("UNRELATED_ARTIFACT")) marker = hash("unrelated artifact");
        if (brief.contract.scope.includes("FALLBACK_WITH_CURRENT_QUESTION")) {
          const question = { schema: 1, identity: brief.identity, result_version: brief.result_version,
            question_id: "fallback-citation", question_version: 1, supersedes: null, kind: "Clarification",
            question: "QUESTION_BODY_FALLBACK_PRIVATE: Which citation label should a later discussion use?",
            context: "The full verbatim README already fulfills this assignment; this optional clarification grants no new authority.",
            choices: ["README", "repository README"] };
          const reply = await host.tool("ovrcr_publish_question", { handle: brief.handle, question });
          toolOutput(`${path}.question-published`, reply);
          pendingQuestion = { path, brief, question,
            reference: reply.details?.questions?.findLast(state => state.current)?.reference };
        }
        // Settle through the actual extension Reporter before displaying evidence.
        if (!brief.contract.scope.includes("FALLBACK_INTERRUPT_RUNNING")) {
          await host.emit({ type: "agent_end", messages: [assistant("stop")] });
          await host.emit({ type: "agent_settled" });
        }
        const line = brief.contract.scope.includes("TRUNCATED_SUCCESS") ? `SUCCESS OVRCR_FALLBACK:${marker.slice(0, 20)}` : `OVRCR_FALLBACK:${marker}`;
        await new Promise(resolve => process.stdout.write(`\n${line}\n`, resolve));
        writeFileSync(`${path}.fallback-ready`, line);
        // File gates drive fixture model work only; rebind and later publication
        // still cross real Reporter/tools and cannot create parent recoveries.
        void (async () => {
          for (let tick = 0; tick < 12000; tick++) {
            if (existsSync(`${path}.rebind`)) {
              unlinkSync(`${path}.rebind`);
              host.state.session = `fallback-replacement-${process.pid}`;
              await host.emit({ type: "session_start", reason: "new" });
              writeFileSync(`${path}.rebound`, "REAL_REPORTER_REBOUND");
            }
            if (existsSync(`${path}.publish-later`)) {
              unlinkSync(`${path}.publish-later`);
              const submission = { schema: 1, identity: brief.identity, version: brief.result_version,
                outcome: "success", summary: "Later genuine child publication", evidence: ["README:1"], unresolved: [] };
              let reply;
              try { reply = await host.tool("ovrcr_publish", { handle: brief.handle, submission }); }
              catch (error) { reply = { error: String(error) }; }
              toolOutput(`${path}.published-later`, reply);
            }
            await new Promise(resolve => setTimeout(resolve, 5));
          }
        })().catch(error => { process.stderr.write(`FALLBACK_FIXTURE_FAILED ${error.message}\n`); process.exitCode = 1; });
        continue;
      }
      if (brief.contract.scope.includes("QUESTION_")) {
        const initial = { schema: 1, identity: brief.identity, result_version: brief.result_version ?? 1, question_id: "readme-choice", question_version: 1, supersedes: null, kind: "Clarification", question: "QUESTION_BODY_SECRET: Which README section is within the authorized scope?", context: "QUESTION_CONTEXT_SECRET: The repository has two sections.", choices: ["Introduction", "Installation"] };
        const questionState = { path, brief, reference: null, question: initial };
        pendingQuestion = questionState;
        const publish = async question => {
          let reply;
          try { reply = await host.tool("ovrcr_publish_question", { handle: brief.handle, question }); }
          catch (error) { reply = { error: String(error) }; }
          const selected = reply.details?.questions?.findLast(state => state.current && state.reference.question_id === question.question_id && state.reference.question_version === question.question_version);
          if (selected) { questionState.reference = selected.reference; questionState.question = question; }
          return reply;
        };
        if (brief.contract.scope.includes("QUESTION_NATIVE_GAP")) await host.emit({ type: "ui_prompt_start", kind: "confirm" });
        else if (!brief.contract.scope.includes("QUESTION_NO_EVENT_GAP")) toolOutput(`${path}.question-published`, await publish(initial));
        // The child must leave readline free to receive the real scoped input. These
        // file gates only drive fixture model work; every publication still crosses
        // the registered child tool, control client, and binary Server.
        const configuredPollMs = process.env.OVRCR_TEST_QUESTION_POLL_MS;
        const pollMs = configuredPollMs === undefined ? 60000 : Number(configuredPollMs);
        if (!Number.isSafeInteger(pollMs) || pollMs < 60000 || pollMs > 1800000) throw new Error("OVRCR_TEST_QUESTION_POLL_MS must be an integer in 60000..1800000");
        const pollDeadline = performance.now() + pollMs;
        void (async () => {
          let resultPublished = false;
          // Keep the original default ticks; native CUA opts into a longer,
          // bounded monotonic lifetime without changing product behavior.
          for (let tick = 0; configuredPollMs === undefined ? tick < 12000 : performance.now() < pollDeadline; tick++) {
            const commandPath = `${path}.question-command`;
            if (existsSync(commandPath)) {
              const action = JSON.parse(readFileSync(commandPath, "utf8"));
              unlinkSync(commandPath);
              let reply;
              if (action.operation === "native-input") { await host.emit({ type: "ui_prompt_start", kind: "confirm" }); reply = { observed: true }; }
              else if (action.operation === "rebind") { const original = host.state.session; for (const session of ["question-replacement", original]) { host.state.session = session; await host.emit({ type: "session_start", reason: "new" }); } reply = { rebound: true }; }
              else reply = await publish(action.question);
              toolOutput(`${path}.question-command-${action.sequence}`, reply);
            }
            if (!resultPublished && existsSync(process.env.OVRCR_TEST_DELEGATION_RESULT_GATE)) {
              const submission = { schema: 1, identity: brief.identity, version: brief.result_version ?? 1, outcome: "success", summary: "Read the assigned repository README.", evidence: ["README:1 fixture"], unresolved: [] };
              let reply;
              try { reply = await host.tool("ovrcr_publish", { handle: brief.handle, submission }); }
              catch (error) { reply = { error: String(error) }; }
              toolOutput(`${path}.published`, reply);
              resultPublished = true;
            }
            await new Promise(resolve => setTimeout(resolve, 5));
          }
        })().catch(error => { process.stderr.write(`QUESTION_FIXTURE_FAILED ${error.message}\n`); process.exitCode = 1; });
        continue;
      }
      while (!existsSync(process.env.OVRCR_TEST_DELEGATION_RESULT_GATE)) await new Promise(resolve => setTimeout(resolve, 10));
      const submission = { schema: 1, identity: brief.identity, version: brief.result_version ?? 1, outcome: "success", summary: "Read the assigned repository README.", evidence: ["README:1 fixture"], unresolved: [] };
      if (brief.contract.scope.startsWith("DEPENDENCY_FIXTURE\n")) {
        dependencyResult(brief,path,submission);
      } else if (!applyReviewFixture(brief, path, submission, host) && brief.contract.coding) {
        if (process.cwd() !== brief.coding_workspace.path) throw new Error("coding child cwd differs from Server-owned workspace");
        writeFileSync(join(process.cwd(), "change.txt"), `${brief.handle}:${submission.version}\n`);
        const checked = spawnSync("/bin/sh", ["-c", "false"], {cwd:process.cwd(),encoding:"utf8"});
        if (checked.error || checked.status !== 1) throw new Error("fixture check did not execute the expected actual failure");
        writeFileSync(`${path}.check.log`, `false exited ${checked.status}\n`);
        submission.outcome = "failed";
        submission.summary = "Isolated fixture edit; actual check failed.";
        submission.evidence = ["change.txt",`${path}.check.log`];
        submission.coding = {checks:[{name:"fixture-failure",command:"false",outcome:"Failed",exit_code:checked.status,evidence:[`${path}.check.log`]},
          {name:"deliberately-unrun",command:null,outcome:"Unrun",exit_code:null,evidence:[]}],candidate:null};
      }
      if (brief.contract.scope.includes("STALE_RESULT")) submission.identity.contract += 1;
      if (previousSubmission) {
        let late;
        try { late = await host.tool("ovrcr_publish", { handle: brief.handle, submission: previousSubmission }); }
        catch (error) { late = {error:String(error)}; }
        writeFileSync(`${path}.late-result`,JSON.stringify(late));
      }
      previousSubmission = structuredClone(submission);
      if (brief.contract.review) writeFileSync(`${path}.review-attempted`,JSON.stringify(submission));
      let result;
      try { result = await host.tool("ovrcr_publish", { handle: brief.handle, submission }); }
      catch (error) { result = { error: String(error) }; }
      const childAcceptance = await host.tool("ovrcr_accept", {handle:brief.handle, identity:brief.identity, result_version:brief.result_version ?? 1, result_digest:"child-cannot-accept", expected_control_epoch:1});
      writeFileSync(`${path}.child-acceptance`, JSON.stringify(childAcceptance));
      toolOutput(`${path}.published`, result);
      process.stdout.write("CHILD_RESULT_PUBLISHED\n");
      if (brief.contract.scope.includes("REBIND_AFTER_RESULT")) {
        while (!existsSync(`${path}.rebind`)) await new Promise(resolve => setTimeout(resolve, 10));
        host.state.session = `replacement-${process.pid}`;
        await host.emit({ type: "session_start", reason: "new" });
        writeFileSync(`${path}.rebound`, "REBOUND");
      }
      if (existsSync(`${path}.hold-settlement`)) {
        writeFileSync(`${path}.settlement-held`, "HELD");
        while (existsSync(`${path}.hold-settlement`)) await new Promise(resolve => setTimeout(resolve, 10));
      }
      if (!brief.contract.scope.includes("NO_READY")) {
        await host.emit({ type: "agent_end", messages: [assistant("stop")] });
        await host.emit({ type: "agent_settled" });
      }
      continue;
    }
    if (command === "abort_tool") {
      pendingTools.get(clean.slice("abort_tool:".length))?.abort();
      continue;
    }
    if (command === "human_marker") { process.stdout.write(`RECEIVED_HUMAN_${a}\n`); continue; }
    if (command === "exit") process.exit(17);
    if (host) {
      if (command === "mode") host.state.mode = a;
      else if (command === "session_ephemeral") {
        host.state.session = a;
        host.state.ephemeral = true;
        await host.emit({ type: "session_start", reason: "new" });
      }
      else if (command === "session_start") {
        if (a) host.state.session = a;
        await host.emit({ type: "session_start", reason: b ?? "startup" });
      } else if (command === "agent_start") await host.emit({ type: "agent_start" });
      else if (command === "model") {
        host.state.model = a ? { id: a, provider: "test" } : undefined;
      } else if (command === "model_select") {
        const previous = host.state.model;
        host.state.model = a ? { id: a, provider: "test" } : undefined;
        await host.emit({
          type: "model_select",
          model: host.state.model,
          previousModel: previous,
          source: "set",
        });
      } else if (command === "agent_end") {
        const messages = a === "none" ? [] : [assistant(a === "ok" ? "stop" : a)];
        await host.emit({ type: "agent_end", messages, ...(b === "continue" ? { willContinue: true } : {}) });
      } else if (command === "ui_prompt_start" || command === "ui_prompt_end") {
        // Exercise queued tools behind slow, serial native reporting without changing tool deadlines.
        const delay = Number(process.env.OVRCR_TEST_PI_PROMPT_DELAY_MS ?? 0);
        if (delay > 0) await new Promise(resolve => setTimeout(resolve, delay));
        // The title is a deliberate secret: no frame may carry it.
        await host.emit({ type: command, reason: "ui_prompt", kind: a, title: "PROMPT_TITLE_SECRET" });
      } else if (command === "tool_approval_requested") {
        // The reason is a deliberate secret: no frame may carry it.
        await host.emit({
          type: "tool_approval_requested", sessionId: host.state.session, toolName: "bash",
          toolCallId: a, reason: "APPROVAL_REASON_SECRET", approvalMode: "always-ask",
        });
      } else if (command === "tool_approval_resolved") {
        const approved = b !== "false";
        await host.emit({
          type: "tool_approval_resolved", sessionId: host.state.session, toolName: "bash",
          toolCallId: a, approved, ...(approved ? {} : { reason: "denied by user" }),
        });
      } else if (command === "foreign_approval") {
        // An in-process task or advisor child: its own session id, never the root's.
        await host.emit({
          type: "tool_approval_requested", sessionId: "other-session", toolName: "bash",
          toolCallId: a, approvalMode: "always-ask",
        });
      } else if (command === "tool_execution_start") {
        await host.emit({
          type: "tool_execution_start", toolCallId: b, toolName: a,
          args: { question: "QUESTION_TEXT_SECRET" }, intent: "ask the user",
        });
      } else if (command === "tool_execution_end") {
        await host.emit({
          // `isError` distinguishes an answered end from a timed-out, aborted or failed
          // one. The extension never reads it: the tool's end closes the request whatever
          // the outcome was.
          type: "tool_execution_end", toolCallId: b, toolName: a,
          result: { text: "ANSWER_SECRET" }, isError: c === "error",
        });
      } else if (command === "session_replace") {
        // Pi replaces the whole extension factory on a session replacement: the running
        // one is shut down and a new instance is created from the same extension path.
        const previous = host.state.session;
        await host.emit({ type: "session_shutdown", reason: b ?? "resume" });
        retiredHost = host;
        host = await createHost(extension, { idle: host.state.idle });
        if (a) host.state.session = a;
        await host.emit({
          type: "session_start", reason: b ?? "resume",
          // Pi names a session file `<timestamp>_<session id>.jsonl`
          // (session-manager.js: `${fileTimestamp}_${this.sessionId}.jsonl`).
          previousSessionFile: `/private/x/2026-09-13T00-00-00-000Z_${previous}.jsonl`,
        });
      } else if (command === "producer_replace") {
        // Simulate a lost producer close. The first announcement pauses; the
        // next authoritative boundary recovers the newly admitted producer.
        retiredHost = host;
        host = await createHost(extension, { session: a });
        await host.emit({ type: "session_start", reason: "reload" });
        await host.emit({ type: "session_start", reason: "reload" });
      } else if (command === "retired_start") {
        if (!retiredHost) throw new Error("no retired producer");
        retiredHost.state.session = a;
        await retiredHost.emit({ type: "session_start", reason: "resume" });
      } else if (command === "forge_workflow_source") {
        const body = { schema: 1, mode: "tui", event: "session_start", reason: "reload", instance: a,
          sequence: Number(b), session_id: host.state.session,
          session_file: host.state.history ?? `${process.env.OVRCR_TEST_HISTORY}/${host.state.session}.jsonl`,
          source_key: Array(32).fill(0x12) };
        // Exercise the real rooted helper; never persist or print its private body.
        execFileSync(process.env.OVRCR_TEST_CONTROL_BINARY, ["report", "pi", "--stdin"], { input: JSON.stringify(body), stdio: ["pipe", "ignore", "ignore"] });
        process.stdout.write("PI_FORGED_SOURCE_ATTEMPTED\n");
      } else if (command === "session_tree") await host.emit({ type: "session_tree" });
      else if (command === "session_compact") {
        await host.emit({ type: "session_before_compact" });
        await host.emit({ type: "session_compact" });
      }
      else if (command === "session_compact_failed") {
        await host.emit({ type: "session_before_compact" });
        await host.emit({ type: "session_compact_failed", aborted: true });
      }
      else if (command === "continuation_context") {
        const messages = await host.emit({ type: "before_agent_start" });
        process.stdout.write(`PI_CONTINUATION=${JSON.stringify(messages)}\n`);
      }
      else if (command === "idle") host.state.idle = a !== "false";
      else if (command === "run_command") {
        await host.run(a);
        // The Rust lifecycle tests watch the terminal for what the command told the user.
        for (const [message, level] of host.state.notices.splice(0)) {
          process.stdout.write(`PI_NOTICE=[${level}] ${message}\n`);
        }
      }
      else if (command === "session_switch") {
        // Oh My Pi switches in place on the same extension instance and names the session
        // file it left; `<id>` equal to the current session is a same-file reload.
        const previous = c ?? host.state.session;
        if (a) host.state.session = a;
        await host.emit({
          type: "session_switch", reason: b ?? "resume",
          previousSessionFile: `/private/x/2026-09-13T00-00-00-000Z_${previous}.jsonl`,
        });
      } else if (command === "plugin_refresh") {
        // Oh My Pi's plugin-resource refresh reloads plugin roots, agents, skills, slash
        // commands and MCP servers and emits no extension event; it never re-instantiates an
        // extension factory. So this host deliberately does nothing, and the tests assert
        // that nothing is what the reporter observes.
      } else if (command === "agent_settled") await host.emit({ type: "agent_settled" });
      else if (command === "session_shutdown") await host.emit({ type: "session_shutdown", reason: a ?? "quit" });
    }
    process.stdout.write(`PI_CALLBACK=${index}\n`);
    index += 1;
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    process.stderr.write(`${error}\n`);
    process.exit(1);
  });
}

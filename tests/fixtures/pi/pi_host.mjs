// Test host for the OVRCR reporting extensions. It serves both harnesses: Pi and Oh My
// Pi share the event vocabulary, and the grammar below covers the events only one of them
// emits (agent_settled for Pi, session_switch and a continuing agent_end for Oh My Pi).
// As a library it fires the real registered handlers with a fake ExtensionContext
// (node --test). Run directly it is a fake `pi` or `omp` executable for the Rust lifecycle
// tests: it loads the `-e` extension and turns stdin lines into extension events, printing
// PI_CALLBACK=<n> after each one.
import { pathToFileURL } from "node:url";
import { createInterface } from "node:readline";
import { writeFileSync, readFileSync, existsSync, appendFileSync, renameSync } from "node:fs";
import { join } from "node:path";

export function assistant(stopReason) {
  return { role: "assistant", stopReason, content: [] };
}

export async function createHost(extensionPath, { mode = "tui", session = "session-a", idle = true, model = undefined } = {}) {
  const handlers = new Map();
  const commands = new Map();
  const tools = new Map();
  const api = {
    on(event, handler) {
      if (!handlers.has(event)) handlers.set(event, []);
      handlers.get(event).push(handler);
    },
    registerTool(tool) { tools.set(tool.name, tool); },
    registerCommand(name, options) {
      commands.set(name, options);
    },
  };
  const module = await import(pathToFileURL(extensionPath).href);
  await module.default(api);
  const state = {
    mode,
    session,
    idle,
    notices: [],
    model: model === undefined ? undefined : { id: model, provider: "test" },
  };
  const ctx = {
    get mode() { return state.mode; },
    hasUI: true,
    sessionManager: {
      getSessionId: () => state.session,
      getSessionFile: () => {
        if (state.ephemeral) return undefined;
        if (!process.env.OVRCR_TEST_HISTORY) return state.history;
        const file = `${process.env.OVRCR_TEST_HISTORY}/${state.session}.jsonl`;
        writeFileSync(file, JSON.stringify({ type: "session", version: 3, id: state.session, cwd: process.cwd() }) + "\n");
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
    tool: (name, args) => tools.get(name).execute("host-tool", args, undefined, undefined, ctx),
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
    host.state.session = `fixture-${process.pid}`;
    host.state.model = { id: args[args.indexOf("--model") + 1] || "fixture-model", provider: "test" };
    if (args.includes("--tools") && process.env.OVRCR_TEST_DELEGATION_INITIAL_MODEL) host.state.model.id = process.env.OVRCR_TEST_DELEGATION_INITIAL_MODEL;
    await host.emit({ type: "session_start", reason: "startup" });
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
  process.stdout.write("PI_NATIVE_READY\n");
  let index = 0;
  let toolPublication = 0;
  const toolOutput = (path, value) => {
    const temporary = `${path}.${process.pid}.${toolPublication++}.tmp`;
    writeFileSync(temporary, JSON.stringify(value));
    renameSync(temporary, path);
  };
  // Start flowing stdin only after startup awaits, immediately before readline.
  // Otherwise an observed binding can prompt input before readline owns it.
  if (process.env.OVRCR_TEST_DELEGATION_BYTES) {
    const session = process.env.OVRCR_DELEGATION_SESSION;
    if (!/^[1-9][0-9]*$/.test(session ?? "")) throw new Error("byte counter requires assigned session identity");
    process.stdin.setRawMode?.(true);
    process.stdin.on("data", bytes => appendFileSync(join(process.env.OVRCR_TEST_DELEGATION_BYTES, session), `${bytes.length}\n`));
  }
  for await (const line of createInterface({ input: process.stdin })) {
    const clean = line.replaceAll("\x1b[200~", "").replaceAll("\x1b[201~", "");
    const [command, a, b, c] = clean.split(":");
    if (host && command === "tool") {
      const request = JSON.parse(clean.slice(5));
      try { toolOutput(request.output, await host.tool(request.name, request.args)); }
      catch (error) { toolOutput(request.output, { error: String(error) }); }
      continue;
    }
    if (host && process.env.OVRCR_TEST_DELEGATION && clean.startsWith("Perform the read-only research assignment in ")) {
      const path = clean.match(/in (.+\/brief-v1\.json)\. Read/)[1];
      const brief = JSON.parse(readFileSync(path, "utf8"));
      writeFileSync(`${path}.started`, "CHILD_RESEARCH_STARTED");
      appendFileSync(`${path}.input-count`, "1\n");
      process.stdout.write("CHILD_RESEARCH_STARTED\n");
      await host.emit({ type: "agent_start" });
      while (!existsSync(process.env.OVRCR_TEST_DELEGATION_RESULT_GATE)) await new Promise(resolve => setTimeout(resolve, 10));
      const submission = { schema: 1, identity: brief.identity, version: 1, outcome: "success", summary: "Read the assigned repository README.", evidence: ["README:1 fixture"], unresolved: [] };
      if (brief.contract.scope.includes("STALE_RESULT")) submission.identity.contract += 1;
      let result;
      try { result = await host.tool("ovrcr_publish", { handle: brief.handle, submission }); }
      catch (error) { result = { error: String(error) }; }
      writeFileSync(`${path}.published`, JSON.stringify(result));
      process.stdout.write("CHILD_RESULT_PUBLISHED\n");
      await host.emit({ type: "agent_end", messages: [assistant("stop")] });
      await host.emit({ type: "agent_settled" });
      continue;
    }
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
      } else if (command === "session_tree") await host.emit({ type: "session_tree" });
      else if (command === "session_compact") await host.emit({ type: "session_compact" });
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

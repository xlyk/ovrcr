// Test host for the OVRCR reporting extensions. It serves both harnesses: Pi and Oh My
// Pi share the event vocabulary, and the grammar below covers the events only one of them
// emits (agent_settled for Pi, session_switch and a continuing agent_end for Oh My Pi).
// As a library it fires the real registered handlers with a fake ExtensionContext
// (node --test). Run directly it is a fake `pi` or `omp` executable for the Rust lifecycle
// tests: it loads the `-e` extension and turns stdin lines into extension events, printing
// PI_CALLBACK=<n> after each one.
import { pathToFileURL } from "node:url";
import { createInterface } from "node:readline";
import { writeFileSync } from "node:fs";

export function assistant(stopReason) {
  return { role: "assistant", stopReason, content: [] };
}

export async function createHost(extensionPath, { mode = "tui", session = "session-a" } = {}) {
  const handlers = new Map();
  const api = {
    on(event, handler) {
      if (!handlers.has(event)) handlers.set(event, []);
      handlers.get(event).push(handler);
    },
  };
  const module = await import(pathToFileURL(extensionPath).href);
  await module.default(api);
  const state = { mode, session };
  const ctx = {
    get mode() { return state.mode; },
    hasUI: true,
    sessionManager: { getSessionId: () => state.session, getSessionFile: () => undefined },
  };
  return {
    state,
    registered: () => [...handlers.keys()].sort(),
    async emit(event) {
      const results = [];
      for (const handler of handlers.get(event.type) ?? []) results.push(await handler(event, ctx));
      return results;
    },
  };
}

// stdin grammar: session_start[:<id>[:<reason>]] | agent_start
//   | agent_end:<ok|error|aborted|none>[:continue] | agent_settled
//   | ui_prompt_start:<kind> | ui_prompt_end:<kind>
//   | tool_approval_requested:<id> | tool_approval_resolved:<id>:<true|false>
//   | foreign_approval:<id> | tool_execution_start:<toolName>:<id>
//   | tool_execution_end:<toolName>:<id>
//   | session_switch[:<id>[:<reason>]] | session_shutdown:<reason>
//   | mode:<tui|rpc|print> | exit
async function main() {
  const args = process.argv.slice(2);
  const at = args.indexOf("-e");
  const host = at >= 0 ? await createHost(args[at + 1]) : null;
  if (process.env.OVRCR_TEST_PROBE) {
    writeFileSync(
      process.env.OVRCR_TEST_PROBE,
      `${process.env.OVRCR_AGENT_SOCKET ?? ""}\n${process.env.OVRCR_AGENT_TOKEN ?? ""}\n${process.pid}\n`,
    );
  }
  process.stdout.write("PI_NATIVE_READY\n");
  let index = 0;
  for await (const line of createInterface({ input: process.stdin })) {
    const [command, a, b] = line.split(":");
    if (command === "exit") process.exit(17);
    if (host) {
      if (command === "mode") host.state.mode = a;
      else if (command === "session_start") {
        if (a) host.state.session = a;
        await host.emit({ type: "session_start", reason: b ?? "startup" });
      } else if (command === "agent_start") await host.emit({ type: "agent_start" });
      else if (command === "agent_end") {
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
          type: "tool_execution_end", toolCallId: b, toolName: a,
          result: { text: "ANSWER_SECRET" }, isError: false,
        });
      } else if (command === "session_switch") {
        if (a) host.state.session = a;
        await host.emit({ type: "session_switch", reason: b ?? "resume" });
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

#!/usr/bin/env node
// Test stub for the ovrcr CLI. Records argv and returns canned JSON.

import { appendFileSync, writeSync } from "node:fs";

const args = process.argv.slice(2);
if (process.env.STUB_LOG) {
  appendFileSync(process.env.STUB_LOG, `${JSON.stringify(args)}\n`);
}

const fail = process.env.STUB_FAIL;
if (fail) {
  const index = fail.indexOf(":");
  const code = index === -1 ? fail : fail.slice(0, index);
  const message = index === -1 ? "stub failure" : fail.slice(index + 1);
  process.stderr.write(`${JSON.stringify({ error: { code, message } })}\n`);
  process.exit(1);
}

// writeSync avoids dropping a >64 KiB payload when the process exits.
const emit = (value) => writeSync(1, `${JSON.stringify(value)}\n`);

const DEFAULT_LIST = [
  {
    id: 12,
    run: 3,
    archived: false,
    project: "demo",
    workspace: "main",
    title: "api-refactor",
    display_name: "api-refactor",
    name: "api-refactor",
    label: "claude / sonnet-4",
    phase: "running",
    activity: "waiting_input",
    unread: null,
    scope: "Workspace",
    started_unix_ms: Date.now() - 120_000,
    agent: {
      binding: { provider: "Claude", invocation: "inv", conversation: "conv", generation: 1 },
      input_requests: [{ id: "approval:1", kind: "Approval" }],
    },
  },
  {
    id: 34,
    run: 1,
    archived: false,
    project: "demo",
    workspace: "billing",
    title: "billing-fix",
    display_name: "billing-fix",
    name: "billing-fix",
    label: "codex / gpt-5.4",
    phase: "running",
    activity: "busy",
    unread: null,
    scope: "Workspace",
    started_unix_ms: Date.now() - 600_000,
    agent: {
      binding: { provider: "Codex", invocation: "inv", conversation: "conv", generation: 1 },
      input_requests: [],
    },
  },
];

if (args.includes("list")) {
  const list = process.env.STUB_LIST ? JSON.parse(process.env.STUB_LIST) : DEFAULT_LIST;
  emit(list);
  process.exit(0);
}

if (args.includes("read")) {
  const index = args.indexOf("read");
  const id = Number(args[index + 1] ?? 0);
  emit({ id, rows: 24, cols: 80, text: `screen for ${id}\n` });
  process.exit(0);
}

function argValue(name, fallback) {
  const hit = args.find((value) => value.startsWith(`--${name}=`));
  return hit ? Number(hit.slice(name.length + 3)) : fallback;
}

function cell(text, fg = "Default", attributes = 0) {
  return { text, width: 1, fg, bg: "Default", attributes };
}

function line(text, fg = "Default", attributes = 0) {
  return {
    width: text.length,
    cells: Array.from(text, (character) => cell(character, fg, attributes)),
    wrapped: false,
  };
}

// A visual fixture: colored tests, a prompt, and a truecolor line.
function screenLine(row) {
  if (row < 120) {
    return line(`  [${String(row).padStart(4, "0")}] retained scrollback row ${row}`, "Default", 2);
  }
  const index = row - 120;
  if (index === 0) return line("$ cargo test -p ovrcr-protocol", { Indexed: 6 }, 1);
  if (index === 1) return line("   Compiling ovrcr-protocol v0.1.0", "Default", 2);
  if (index === 2) return line("    Finished dev profile in 2.14s", { Indexed: 2 });
  if (index === 3) return line("     Running unittests src/lib.rs");
  if (index === 4) return line("running 101 tests");
  if (index === 39) return line("$ ", { Indexed: 6 }, 1);
  if (index === 38) return line("done", { Rgb: [122, 162, 247] }, 1);
  if (index === 37) return line('$ echo "done"', { Indexed: 6 }, 1);
  if (index === 36) return line("?? experiments/remote-web/", { Indexed: 1 });
  if (index === 35) return line(" M src/remote/server.rs", { Indexed: 3 });
  if (index === 34) return line("$ git status --short", { Indexed: 6 }, 1);
  if (index === 33) return line("test result: ok. 101 passed; 0 failed", { Indexed: 2 }, 1);
  if (index === 32) return line("");
  return line(`test wire::snapshot_${String(index).padStart(2, "0")} ... ok`, { Indexed: 2 });
}

if (process.env.STUB_OLD && args.includes("screen")) {
  process.stderr.write("error: unrecognized subcommand 'screen'\n\nUsage: ovrcr terminal <COMMAND>\n");
  process.exit(2);
}

if (args.includes("screen")) {
  const index = args.indexOf("screen");
  const id = Number(args[index + 1] ?? 0);
  const start = argValue("start-row", 0);
  const count = argValue("rows", 40);
  const cols = argValue("cols", 200);
  const total = 160;
  const rows = [];
  for (let row = start; row < Math.min(start + count, total); row += 1) {
    rows.push(screenLine(row));
  }
  emit({
    session: id,
    run: 3,
    size: { rows: 40, cols: Math.min(cols, 80) },
    history_rows: 120,
    total_rows: total,
    start_row: start,
    alternate_screen: false,
    cursor: { row: 39, col: 2, hidden: false },
    rows,
  });
  process.exit(0);
}

if (args.includes("transcript")) {
  const index = args.indexOf("transcript");
  const id = Number(args[index + 1] ?? 0);
  emit({
    entries: [
      { User: "Why is the auth handoff losing the refresh token?" },
      {
        Assistant:
          "I found it. The handoff reads the token before the refresh completes, so the retry path reuses a rotated secret.",
      },
      { Tool: { name: "Read", input: "src/auth/handoff.rs" } },
      {
        Assistant:
          "Swapping the read and refresh order fixes it. I added a regression test for the rotated-token path.",
      },
    ],
    start: 42,
    identity: {
      Claude: {
        conversation: `fixture-${id}`,
        executable: "/claude",
        history: "/history.jsonl",
        config_dir: "/config",
        options: [],
      },
    },
  });
  process.exit(0);
}

emit({ ok: true });

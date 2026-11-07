#!/usr/bin/env node
// Test stub for the ovrcr CLI. Records argv and returns canned JSON.

import { appendFileSync } from "node:fs";

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

const emit = (value) => process.stdout.write(`${JSON.stringify(value)}\n`);

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

emit({ ok: true });

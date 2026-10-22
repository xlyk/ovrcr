// Exercise the installed host validator and the actual registered callback, without a helper process.
import assert from "node:assert/strict";
import childProcess from "node:child_process";
import { syncBuiltinESMExports } from "node:module";
import { EventEmitter } from "node:events";
import { PassThrough } from "node:stream";
import { join } from "node:path";
import { pathToFileURL } from "node:url";

export async function assertPublishContract(publisher, piRoot) {
  const { validateToolArguments } = await import(pathToFileURL(
    join(piRoot, "node_modules/@earendil-works/pi-ai/dist/index.js")).href);
  const identity = { instance: "instance", workflow: "workflow", controller: "controller", controller_epoch: 1,
    task: "task", attempt: 1, contract: 1, request: "request", session: 41, run: 1 };
  const submission = { schema: 1, identity, version: 1, outcome: "success", summary: "Verified the requested sources.",
    evidence: ["README.md:1"], unresolved: [] };
  const args = { handle: "assignment", submission };
  const validate = value => validateToolArguments(publisher, { name: publisher.name, arguments: value });
  const spawn = childProcess.spawn;
  const before = { ...process.env };
  const requests = [];
  try {
    childProcess.spawn = (_binary, argv) => {
      assert.deepEqual(argv, ["--json", "delegation"]);
      const child = new EventEmitter();
      child.stdout = new PassThrough(); child.stderr = new PassThrough(); child.stdin = new PassThrough();
      child.kill = () => { throw new Error("publication must settle without killing a process"); };
      let bytes = "";
      child.stdin.on("data", data => { bytes += data; });
      child.stdin.on("finish", () => {
        requests.push(JSON.parse(bytes));
        child.stdout.write(JSON.stringify({ published: true }));
        child.emit("close", 0);
      });
      return child;
    };
    syncBuiltinESMExports();
    Object.assign(process.env, { OVRCR_DELEGATION_SESSION: "41", OVRCR_DELEGATION_TOKEN: "fixture-not-authority",
      OVRCR_DELEGATION_SOCKET: "/fixture/not-a-channel" });
    const invoke = async value => publisher.execute("publish-contract", validate(value), undefined, undefined, {});
    // RED on the original registration: the installed SDK accepts these incomplete objects.
    for (const field of Object.keys(submission)) {
      const missing = structuredClone(args); delete missing.submission[field];
      await assert.rejects(() => invoke(missing), /Validation failed for tool/, `missing ${field}`);
    }
    const malformed = [
      { ...args, extra: true }, { ...args, submission: { ...submission, extra: true } },
      ...[0, -1, 1.5, 4294967296].map(version => ({ ...args, submission: { ...submission, version } })),
      ...[0, 2].map(schema => ({ ...args, submission: { ...submission, schema } })),
      { ...args, submission: { ...submission, outcome: "Passed" } },
      { ...args, submission: { ...submission, identity: {} } },
      { ...args, submission: { ...submission, identity: { ...identity, controller_epoch: 0 } } },
      { ...args, submission: { ...submission, identity: { ...identity, session: -1 } } },
      { ...args, submission: { ...submission, summary: "" } },
      { ...args, submission: { ...submission, summary: "a".repeat(65537) } },
      { ...args, submission: { ...submission, evidence: [] } },
      { ...args, submission: { ...submission, evidence: Array(65).fill("source") } },
      { ...args, submission: { ...submission, unresolved: ["a".repeat(4097)] } },
      { ...args, submission: { ...submission, coding: [] } },
      { ...args, submission: { ...submission, review: [] } },
    ];
    for (const invalid of malformed) await assert.rejects(() => invoke(invalid), /Validation failed for tool/);
    assert.equal(requests.length, 0, "malformed arguments must have no transport effects");
    assert.deepEqual(publisher.parameters.properties.submission.required,
      ["schema", "identity", "version", "outcome", "summary", "evidence", "unresolved"]);
    // JSON Schema string lengths count characters. Rust owns exact UTF-8 byte
    // limits and Unicode trim semantics; avoid claiming SDK validation proves them.
    for (const summary of ["   ", "é".repeat(65536)]) {
      const value = { ...args, submission: { ...submission, summary } };
      assert.deepEqual(validate(value), value);
    }
    assert.deepEqual(validate(args), args, "research arguments are not normalized or rewritten");
    assert.deepEqual((await invoke(args)).details, { published: true });
    assert.deepEqual(requests[0].Publish.submission, submission, "registered publisher forwards research unchanged");
    assert.equal(requests[0].Publish.handle, args.handle);
    // Nullable/optional extensions and actual nested coding/review payloads remain compatible.
    const digest = "a".repeat(64);
    const coding = { candidate: null, checks: [{ name: "check", command: "node --check example.js", outcome: "Passed",
      exit_code: 0, evidence: ["check.log"] }] };
    const review = { axis: "Spec", outcome: "Passed", findings: [], target: { coding: { assignment: identity,
      publication: { assignment: identity, kind: "Submission", version: 1, digest },
      candidate: { path: "/fixture/candidate", digest, workspace: "coding", base: "a".repeat(40),
        git_identity: "fixture", changed_artifacts: ["README.md"] }, source_contract_digest: digest },
      sources: [{ kind: "Spec", reference: "fixture:spec", digest }] } };
    for (const extension of [{ coding: null, review: null }, { coding }, { review },
      { outcome: "blocked", evidence: [], unresolved: ["Missing source."] }, { outcome: "failed", evidence: [] }]) {
      const value = { ...args, submission: { ...submission, ...extension } };
      assert.deepEqual(validate(value), value);
      await invoke(value);
      assert.deepEqual(requests.at(-1).Publish.submission, value.submission);
    }
    assert.equal(requests.length, 6);
  } finally {
    childProcess.spawn = spawn; syncBuiltinESMExports(); process.env = before;
  }
}

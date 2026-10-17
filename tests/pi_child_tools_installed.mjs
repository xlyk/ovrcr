// Actual assignment-launch argv, materialized runtime, and installed AgentSession; no inference.
import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, realpathSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, isAbsolute } from "node:path";
import { pathToFileURL } from "node:url";

const piRoot = process.env.OVRCR_TEST_PI_PACKAGE_ROOT;
const capture = process.env.OVRCR_TEST_PI_CHILD_ARGV;
assert.ok(piRoot && isAbsolute(piRoot) && capture && isAbsolute(capture));
const metadata = JSON.parse(readFileSync(join(piRoot, "package.json"), "utf8"));
assert.equal(metadata.name, "@earendil-works/pi-coding-agent");
assert.equal(metadata.version, "0.87.1");
const launches = JSON.parse(readFileSync(capture, "utf8"));
assert.deepEqual(launches.map(launch => launch.kind), ["research", "coding"]);
const sdk = await import(pathToFileURL(join(piRoot, "dist/index.js")).href);

for (const { kind, argv } of launches) {
  test(`production ${kind} argv exposes child publishers in the installed effective catalog`, async () => {
    const root = mkdtempSync(join(tmpdir(), "ovrcr-child-catalog-"));
    const agentDir = join(root, "agent");
    mkdirSync(agentDir, { mode: 0o700 });
    const before = { ...process.env };
    let session;
    try {
      process.env.HOME = root;
      process.env.PI_CODING_AGENT_DIR = agentDir;
      const parsed = sdk.parseArgs(argv);
      assert.ok(parsed.tools?.length, "use actual production --tools via the installed CLI parser");
      assert.equal(parsed.extensions.length, 1, "one actual production injection");
      const injected = realpathSync(parsed.extensions[0]);
      Object.assign(process.env, {
        OVRCR_AGENT_SOCKET: "/fixture/not-a-channel", OVRCR_AGENT_TOKEN: "fixture-not-authority",
        OVRCR_PI_ENTRY: JSON.stringify({ schema: 1, path: injected }),
      });
      const settings = sdk.SettingsManager.create(root, agentDir);
      const resourceLoader = new sdk.DefaultResourceLoader({ cwd: root, agentDir, settingsManager: settings,
        additionalExtensionPaths: [injected], noExtensions: true, noSkills: true,
        noContextFiles: true, noPromptTemplates: true, noThemes: true });
      await resourceLoader.reload();
      assert.deepEqual(resourceLoader.getExtensions().errors, []);
      const registered = resourceLoader.getExtensions().extensions.flatMap(extension => [...extension.tools.keys()]);
      for (const name of ["ovrcr_publish", "ovrcr_publish_question"]) assert.ok(registered.includes(name));
      const modelRuntime = await sdk.ModelRuntime.create({ authPath: join(agentDir, "auth.json"),
        modelsPath: null, refreshOnCreate: false, allowModelNetwork: false });
      ({ session } = await sdk.createAgentSession({ cwd: root, agentDir, settingsManager: settings,
        resourceLoader, modelRuntime, sessionManager: sdk.SessionManager.inMemory(root), tools: parsed.tools }));
      const active = session.getActiveToolNames().sort();
      const configured = session.getAllTools().map(tool => tool.name).sort();
      console.log(JSON.stringify({ kind, launchTools: parsed.tools, active, configured }));
      const expected = ["read", "grep", "find", "ls", "ovrcr_publish", "ovrcr_publish_question",
        ...(kind === "coding" ? ["write", "edit", "bash"] : [])].sort();
      assert.deepEqual(active, expected, "effective tools must preserve publication and the exact child boundary");
      assert.deepEqual(configured, expected, "disallowed tools cannot be re-enabled from the registry");
      assert.deepEqual(active.filter(name => name.startsWith("ovrcr_")), ["ovrcr_publish", "ovrcr_publish_question"]);
    } finally {
      session?.dispose();
      process.env = before;
      rmSync(root, { recursive: true, force: true });
    }
  });
}

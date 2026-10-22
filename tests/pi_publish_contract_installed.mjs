// Offline installed-SDK gate using source-materialized runtime; native launch qualification is separate.
import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, rmSync } from "node:fs";
import { join, isAbsolute } from "node:path";
import { tmpdir } from "node:os";
import { pathToFileURL } from "node:url";
import { assertPublishContract } from "./fixtures/pi/pi_publish_contract.mjs";
const piRoot = process.env.OVRCR_TEST_PI_PACKAGE_ROOT;
assert.ok(piRoot && isAbsolute(piRoot));
const installed = JSON.parse(readFileSync(join(piRoot, "package.json"), "utf8"));
assert.equal(installed.name, "@earendil-works/pi-coding-agent");
assert.equal(installed.version, "0.87.1");
const sdk = await import(pathToFileURL(join(piRoot, "dist/index.js")).href);

test("installed registered publisher validates discoverable research contract before transport and forwards valid reports", async () => {
  const root = mkdtempSync(join(tmpdir(), "ovrcr-publish-contract-"));
  const before = { ...process.env };
  try {
    const agentDir = join(root, "agent"); mkdirSync(agentDir, { mode: 0o700 });
    process.env.HOME = root; process.env.PI_CODING_AGENT_DIR = agentDir;
    Object.assign(process.env, { OVRCR_AGENT_SOCKET: "/fixture/not-a-channel", OVRCR_AGENT_TOKEN: "fixture-not-authority" });
    for (const file of ["ovrcr-pi-runtime.ts", "ovrcr-pi-interrupt.ts", "ovrcr-reporting-transport.mjs",
      "ovrcr-delegation-observer.mjs", "ovrcr-delegation-continuation.mjs"]) {
      writeFileSync(join(root, file), readFileSync(new URL(`../src/${file}`, import.meta.url)));
    }
    const entry = join(root, "reporting.ts");
    writeFileSync(entry, readFileSync(new URL("../src/pi-reporting-extension.mjs", import.meta.url), "utf8")
      .replace("__OVRCR_BINARY__", JSON.stringify("/fixture/no-native-execution")));
    const loader = new sdk.DefaultResourceLoader({ cwd: root, agentDir,
      settingsManager: sdk.SettingsManager.create(root, agentDir), additionalExtensionPaths: [entry],
      noExtensions: true, noSkills: true, noContextFiles: true, noPromptTemplates: true, noThemes: true });
    await loader.reload();
    assert.deepEqual(loader.getExtensions().errors, []);
    const owners = loader.getExtensions().extensions.filter(extension => extension.tools.has("ovrcr_publish"));
    assert.equal(owners.length, 1);
    await assertPublishContract(owners[0].tools.get("ovrcr_publish").definition, piRoot);
  } finally { process.env = before; rmSync(root, { recursive: true, force: true }); }
});

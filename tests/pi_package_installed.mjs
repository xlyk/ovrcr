// Explicit installed-loader gate. This suite deliberately fails without its installed
// Pi root and real production-materialized entry; it never turns absent evidence green.
import { test } from "node:test";
import { createHash } from "node:crypto";
import assert from "node:assert/strict";
import { mkdtempSync, mkdirSync, readFileSync, rmSync, realpathSync, existsSync } from "node:fs";
import { join, resolve, dirname, isAbsolute } from "node:path";
import { tmpdir } from "node:os";
import { fileURLToPath, pathToFileURL } from "node:url";

const repository = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const piRoot = process.env.OVRCR_TEST_PI_PACKAGE_ROOT;
assert.ok(piRoot && isAbsolute(piRoot), "set OVRCR_TEST_PI_PACKAGE_ROOT to the actual installed Pi package");
const installed = JSON.parse(readFileSync(join(piRoot, "package.json"), "utf8"));
assert.equal(installed.name, "@earendil-works/pi-coding-agent");
assert.equal(installed.version, "0.87.1", "the dedicated loader gate uses the pinned installed host");

async function fixture(run) {
  const before = { ...process.env };
  const root = mkdtempSync(join(tmpdir(), "ovrcr-installed-package-"));
  const cwd = join(root, "workspace");
  const agentDir = join(root, "agent");
  mkdirSync(cwd); mkdirSync(agentDir, { mode: 0o700 });
  try {
    for (const key of Object.keys(process.env)) {
      if (/^(OVRCR_|PI_CODING_AGENT_|XDG_)/.test(key)) delete process.env[key];
    }
    process.env.HOME = root;
    process.env.PI_CODING_AGENT_DIR = agentDir;
    const { DefaultResourceLoader, DefaultPackageManager, SettingsManager } =
      await import(pathToFileURL(join(piRoot, "dist/index.js")).href);
    const settings = SettingsManager.create(cwd, agentDir);
    const manager = new DefaultPackageManager({ cwd, agentDir, settingsManager: settings });
    await manager.installAndPersist(repository);
    const makeLoader = additionalExtensionPaths => new DefaultResourceLoader({
      cwd, agentDir, settingsManager: settings, additionalExtensionPaths,
      noContextFiles: true, noPromptTemplates: true, noThemes: true,
    });
    await run({ root, cwd, agentDir, manager, settings, makeLoader });
  } finally {
    process.env = before;
    rmSync(root, { recursive: true, force: true });
  }
}

test("local installed package discovers one valid promoted skill and confined links", async () => {
  await fixture(async ({ agentDir, manager, makeLoader }) => {
    const loader = makeLoader([]);
    await loader.reload();
    assert.deepEqual(loader.getExtensions().errors, []);
    const { skills, diagnostics } = loader.getSkills();
    assert.deepEqual(diagnostics, []);
    assert.deepEqual(skills.map(skill => skill.name), ["ovrcr-orchestrator"]);
    assert.equal(skills[0].disableModelInvocation, false);
    const skillRoot = join(repository, "pi/skills/ovrcr-orchestrator");
    assert.equal(realpathSync(skills[0].baseDir), realpathSync(skillRoot));
    for (const file of ["SKILL.md", ...["delegation", "implementation", "records", "workflow"].map(name => `references/${name}.md`)]) {
      const path = join(skillRoot, file);
      for (const match of readFileSync(path, "utf8").matchAll(/\[[^\]]+\]\(([^)]+)\)/g)) {
        if (/^(?:https?:|#)/.test(match[1])) continue;
        const target = realpathSync(resolve(dirname(path), match[1].split("#")[0]));
        assert.ok(target.startsWith(realpathSync(repository) + "/"), `link escapes package repository: ${file}`);
      }
    }
    assert.ok(existsSync(join(agentDir, "settings.json")), "real package install persists only selected profile settings");
    assert.deepEqual(manager.listConfiguredPackages().map(pkg => realpathSync(pkg.source)), [realpathSync(repository)]);
    assert.equal(await manager.removeAndPersist(repository), true);
    const removed = makeLoader([]);
    await removed.reload();
    assert.deepEqual(removed.getSkills().skills, []);
  });
});

test("installed package plus real injected entry keeps one runtime registration owner across reload", async () => {
  const injected = process.env.OVRCR_TEST_PI_MATERIALIZED_ENTRY;
  assert.ok(injected && isAbsolute(injected) && existsSync(injected),
    "set OVRCR_TEST_PI_MATERIALIZED_ENTRY to an entry retained from the production OVRCR materializer");
  await fixture(async ({ makeLoader }) => {
    Object.assign(process.env, {
      OVRCR_AGENT_SOCKET: "/fixture/not-a-channel", OVRCR_AGENT_TOKEN: "fixture-not-authority",
      OVRCR_PI_ENTRY: JSON.stringify({ schema: 1, path: injected }),
    });
    const loader = makeLoader([injected]);
    await loader.reload();
    let { extensions, errors } = loader.getExtensions();
    assert.deepEqual(errors, []);
    assert.equal(extensions.length, 2, "package and normal injection must both remain discovered");
    const runtimeOwners = extensions.filter(extension => extension.commands.has("ovrcr-reattach"));
    assert.equal(runtimeOwners.length, 1, "first-tool-name-wins cannot hide duplicate lifecycle factories");
    assert.equal(realpathSync(runtimeOwners[0].resolvedPath), realpathSync(injected));
    assert.equal(extensions.filter(extension => extension.tools.has("ovrcr_start")).length, 1);
    assert.equal(extensions.filter(extension => extension.tools.has("ovrcr_dependencies")).length, 1);
    assert.equal(extensions.filter(extension => extension.tools.has("ovrcr_review")).length, 1);
    assert.equal(extensions.filter(extension => extension.handlers.has("agent_settled")).length, 1);
    assert.equal(extensions.find(extension => !extension.tools.size).handlers.get("session_start").length, 1,
      "package entry contributes only its availability diagnostic");
    const old = runtimeOwners[0];
    // Use the installed host's own validator, including its plain-JSON schema handling.
    const { validateToolArguments } = await import(pathToFileURL(
      join(piRoot, "node_modules/@earendil-works/pi-ai/dist/index.js")
    ).href);
    const start = old.tools.get("ovrcr_start").definition;
    const research = { project: "p", workspace: "w", scope: "Read", acceptance_evidence: ["citation"] };
    const validate = arguments_ => validateToolArguments(start, { name: "ovrcr_start", arguments: arguments_ });
    assert.deepEqual(validate({ handle: "original" }), { handle: "original" });
    assert.deepEqual(validate(research), research);
    assert.deepEqual(validate({ ...research, permitted_actions: ["read"], mandatory_capabilities: ["read", "result_file"] }),
      { ...research, permitted_actions: ["read"], mandatory_capabilities: ["read", "result_file"] });
    const coding = { ...research, coding: { requested_base: "main", branch: "coding", authorized_paths: ["README.md"],
      required_checks: ["heading-only"] }, permitted_actions: ["read", "write", "edit", "check"] };
    assert.deepEqual(validate(coding), coding);
    const assignment = { instance: "i", workflow: "workflow", controller: "controller", controller_epoch: 1,
      task: "implementation", attempt: 1, contract: 1, request: "implementation", session: 41, run: 1 };
    const digest = "a".repeat(64);
    const content = "Preserve the README remainder.";
    const source = { kind: "Spec", reference: "fixture:spec", digest: createHash("sha256").update(content).digest("hex") };
    const review = { ...research, permitted_actions: ["read"], review: { implementation: "implementation", axis: "Spec",
      target: { coding: { assignment, publication: { assignment, kind: "Submission", version: 1, digest },
        candidate: { path: "/fixture/candidate", digest, workspace: "coding", base: "a".repeat(40),
          git_identity: "fixture", changed_artifacts: ["README.md"] }, source_contract_digest: digest }, sources: [source] },
      source_documents: [{ source, content }] } };
    assert.deepEqual(validate(review), review, "accepted typed independent reviews keep their public schema");
    for (const invalid of [{ ...research, handle: "original" }, { handle: "original", request: "new" },
      { ...research, permitted_actions: ["read", "result_file"] }]) {
      assert.throws(() => validate(invalid), /Validation failed for tool/);
    }
    const retiredTool = old.tools.get("ovrcr_inspect").definition;
    // A print context prevents report transport; these are the actual loaded shutdown handlers.
    for (const handler of old.handlers.get("session_shutdown") ?? []) {
      await handler({ type: "session_shutdown", reason: "reload" }, { mode: "print" });
    }
    await loader.reload();
    ({ extensions, errors } = loader.getExtensions());
    assert.deepEqual(errors, []);
    assert.equal(extensions.filter(extension => extension.tools.has("ovrcr_start")).length, 1);
    assert.equal(extensions.filter(extension => extension.tools.has("ovrcr_dependencies")).length, 1);
    assert.equal(extensions.filter(extension => extension.tools.has("ovrcr_review")).length, 1);
    assert.notEqual(extensions.find(extension => extension.commands.has("ovrcr-reattach")), old);
    await assert.rejects(() => retiredTool.execute("retired", { handle: "original" }, undefined, undefined, {}),
      /Pi session changed|retired|previous Pi context/);
  });
});

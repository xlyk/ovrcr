import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, writeFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { createHost } from "./fixtures/pi/pi_host.mjs";

test("managed Pi exposes research assignment and explicit collection tools", async () => {
  const dir = mkdtempSync(join(tmpdir(), "ovrcr-delegation-"));
  const before = { ...process.env };
  try {
    process.env.OVRCR_AGENT_SOCKET = "/fixture/socket";
    process.env.OVRCR_AGENT_TOKEN = "fixture-token";
    const source = readFileSync(new URL("../src/pi-reporting-extension.mjs", import.meta.url), "utf8").replace("__OVRCR_BINARY__", JSON.stringify("/fixture/ovrcr"));
    writeFileSync(join(dir, "reporting.mjs"), source);
    writeFileSync(join(dir, "ovrcr-reporting-transport.mjs"), readFileSync(new URL("../src/ovrcr-reporting-transport.mjs", import.meta.url)));
    const host = await createHost(join(dir, "reporting.mjs"));
    assert.deepEqual(host.tools(), ["ovrcr_collect", "ovrcr_inspect", "ovrcr_publish", "ovrcr_start"]);
    assert.deepEqual(host.toolDefinition("ovrcr_publish").parameters.required, ["handle", "submission"]);
    assert.deepEqual(host.toolDefinition("ovrcr_collect").parameters.required, ["handle"]);
    assert.deepEqual(host.toolDefinition("ovrcr_start").parameters.anyOf, [{ required: ["handle"] }, { required: ["project", "workspace", "scope", "model_provider", "model", "acceptance_evidence"] }]);
    const missing = await host.tool("ovrcr_start", {});
    assert.equal(missing.details.dispatch, "Refused");
    assert.equal(missing.details.handle, null);
    await assert.rejects(() => host.tool("ovrcr_inspect", {}), /missing required argument: handle/);
  } finally {
    process.env = before;
    rmSync(dir, { recursive: true, force: true });
  }
});

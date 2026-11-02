import { test } from "node:test";
import assert from "node:assert/strict";
import { backend } from "../app/src/platform/backend/index.ts";
import { DESKTOP_UPDATES_ONLY, UNINSTALL_KEEP, UNINSTALL_PURGE } from "../app/src/platform/updates.ts";

test("demo checks explain the desktop boundary and cannot install", async () => {
  const demo = backend(true);
  assert.deepEqual(await demo.checkUpdate(), { release: null, message: DESKTOP_UPDATES_ONLY });
  assert.equal(await demo.pendingUpdate(), null);
  await assert.rejects(demo.installUpdate("test_1-20261102-123abcd"), { message: DESKTOP_UPDATES_ONLY });
  assert.equal((await demo.about()).commit, "");
});

test("uninstall commands use the registered uninstaller with quoted paths and explicit opt-in", () => {
  assert.ok(UNINSTALL_KEEP.includes("UninstallString.Trim('\"')"));
  assert.ok(UNINSTALL_KEEP.includes("-ArgumentList '/S'"));
  assert.ok(!UNINSTALL_KEEP.includes("/PURGE"));
  assert.equal(UNINSTALL_PURGE, UNINSTALL_KEEP.replace("'/S'", "'/S /PURGE'"));
});

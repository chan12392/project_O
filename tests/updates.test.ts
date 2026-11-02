import { test } from "node:test";
import assert from "node:assert/strict";
import { backend } from "../app/src/platform/backend/index.ts";
import { DESKTOP_UPDATES_ONLY, UNINSTALL_KEEP, UNINSTALL_PURGE, updateStatusLine } from "../app/src/platform/updates.ts";

test("the gh requirement and a failed background check show before anyone presses check", async () => {
  assert.match(updateStatusLine({ automatic: true, gh: false, lastCheck: null }), /gh auth login/);
  const failed = updateStatusLine({ automatic: true, gh: true, lastCheck: { at: "2026-11-02T01:00:00Z", tag: null, error: "릴리스에 접근하지 못했습니다" } });
  assert.match(failed, /실패했습니다: 릴리스에 접근하지 못했습니다/);
  assert.match(updateStatusLine({ automatic: true, gh: true, lastCheck: { at: "2026-11-02T01:00:00Z", tag: null, error: null } }), /새 버전 없음/);
  assert.match(updateStatusLine({ automatic: true, gh: true, lastCheck: { at: "2026-11-02T01:00:00Z", tag: "test_1-20261102-1234567", error: null } }), /test_1-20261102-1234567/);
  assert.match(updateStatusLine({ automatic: false, gh: true, lastCheck: null }), /자동으로 확인하지 않습니다/);
  // Outside the desktop app the screen says nothing about gh.
  assert.deepEqual(await backend(true).updateStatus(), { automatic: false, gh: null, lastCheck: null });
});

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

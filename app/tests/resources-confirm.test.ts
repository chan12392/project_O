import assert from "node:assert/strict";
import { test } from "node:test";
import { devBackend } from "../src/platform/backend/dev.ts";
import { tauriBackend } from "../src/platform/backend/tauri.ts";

test("recovery sends explicit confirmation only when requested", async () => {
  const original = globalThis.fetch;
  const urls: string[] = [];
  globalThis.fetch = async (input) => {
    urls.push(String(input));
    return new Response(JSON.stringify({ ok: true }));
  };
  try {
    await devBackend.recoverResource("7");
    await devBackend.recoverResource("7", true);
    assert.deepEqual(urls, ["/api/resources/recover?id=7", "/api/resources/recover?id=7&confirmExited=1"]);
  } finally { globalThis.fetch = original; }

  const calls: unknown[] = [];
  const desktop = tauriBackend(async (command, args) => { calls.push({ command, args }); });
  await desktop.recoverResource("7");
  await desktop.recoverResource("7", true);
  assert.deepEqual(calls, [
    { command: "resource_recover", args: { id: "7", confirmExited: false } },
    { command: "resource_recover", args: { id: "7", confirmExited: true } },
  ]);
});

import assert from "node:assert/strict";
import { test } from "node:test";
import { devBackend } from "../app/src/platform/backend/dev.ts";
import { tauriBackend } from "../app/src/platform/backend/tauri.ts";

test("resource inspection and recovery use the development server API", async () => {
  const original = globalThis.fetch;
  const calls: { url: string; method: string }[] = [];
  globalThis.fetch = async (input, init) => {
    const url = String(input);
    calls.push({ url, method: init?.method ?? "GET" });
    return new Response(JSON.stringify(url === "/api/resources" ? { leases: [{ id: "7", runId: "r", cardId: "c", spawnIntent: true, exitConfirmed: false, processesExited: true, state: "unknown" }] } : { ok: true }), { status: 200 });
  };
  try {
    const leases = await devBackend.resources();
    assert.equal(leases[0].id, "7");
    assert.equal(leases[0].state, "unknown");
    assert.equal(leases[0].processesExited, true);
    await devBackend.recoverResource(leases[0].id);
    await devBackend.recoverResource(leases[0].id, true);
    assert.deepEqual(calls, [{ url: "/api/resources", method: "GET" }, { url: "/api/resources/recover?id=7", method: "POST" }, { url: "/api/resources/recover?id=7&confirmExited=1", method: "POST" }]);
  } finally { globalThis.fetch = original; }
});

test("desktop recovery passes the explicit confirmation", async () => {
  const calls: unknown[] = [];
  const desktop = tauriBackend(async (command, args) => { calls.push({ command, args }); });
  await desktop.recoverResource("7");
  await desktop.recoverResource("7", true);
  assert.deepEqual(calls, [
    { command: "resource_recover", args: { id: "7", confirmExited: false } },
    { command: "resource_recover", args: { id: "7", confirmExited: true } },
  ]);
});

test("resource recovery shows the server's refusal", async () => {
  const original = globalThis.fetch;
  globalThis.fetch = async () => new Response(JSON.stringify({ error: "실행 중인 예약은 복구할 수 없습니다" }), { status: 500 });
  try {
    await assert.rejects(devBackend.recoverResource("7"), /실행 중인 예약은 복구할 수 없습니다/);
  } finally { globalThis.fetch = original; }
});

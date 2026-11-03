import assert from "node:assert/strict";
import { test } from "node:test";
import { devBackend } from "../app/src/platform/backend/dev.ts";

test("resource inspection and recovery use the development server API", async () => {
  const original = globalThis.fetch;
  const calls: { url: string; method: string }[] = [];
  globalThis.fetch = async (input, init) => {
    const url = String(input);
    calls.push({ url, method: init?.method ?? "GET" });
    return new Response(JSON.stringify(url === "/api/resources" ? { leases: [{ id: "7", runId: "r", cardId: "c", spawnIntent: false }] } : { ok: true }), { status: 200 });
  };
  try {
    const leases = await devBackend.resources();
    assert.equal(leases[0].id, "7");
    await devBackend.recoverResource(leases[0].id);
    assert.deepEqual(calls, [{ url: "/api/resources", method: "GET" }, { url: "/api/resources/recover?id=7", method: "POST" }]);
  } finally { globalThis.fetch = original; }
});

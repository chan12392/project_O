import assert from "node:assert/strict";
import { test } from "node:test";
import { devBackend } from "../app/src/platform/backend/dev.ts";
import { tauriBackend } from "../app/src/platform/backend/tauri.ts";

test("both backends submit the displayed plan and mode without reconstructing the cards", async () => {
  const plan = { title: "검토한 계획", project: "C:/fixture", cards: [{ id: "a", provider: "claude", model: "sonnet", effort: "medium", task: "작업", resources: [{ kind: "external", key: "fixture", mode: "read" }] }] };
  const calls: unknown[] = [];
  const desktop = tauriBackend(async (command, args) => { calls.push([command, args]); return {}; });
  await desktop.leadDispatch("plan-1", false, true, plan);
  assert.deepEqual(calls, [["lead_dispatch", { planId: "plan-1", clean: false, yolo: true, expectedPlan: plan }]]);
  const original = globalThis.fetch;
  globalThis.fetch = async (url, options) => {
    const query = new URL(String(url), "http://localhost");
    assert.equal(query.pathname, "/api/lead/dispatch");
    assert.equal(query.searchParams.get("mode"), "normal");
    assert.equal(query.searchParams.get("clean"), "1");
    assert.equal(options?.method, "POST");
    assert.deepEqual(JSON.parse(String(options?.body)), plan);
    return new Response("{}");
  };
  try { await devBackend.leadDispatch("plan-1", true, false, plan); }
  finally { globalThis.fetch = original; }
});

import assert from "node:assert/strict";
import { test } from "node:test";
import { devBackend } from "../app/src/platform/backend/dev.ts";
import { tauriBackend } from "../app/src/platform/backend/tauri.ts";

test("desktop and development adapters return the same read-only changes", async () => {
  const expected = { workspace: "C:/temp/1", copyBase: "C:/temp/base", baselineRunId: "run-original", changes: [{ path: "a.txt", kind: "modified" }], note: "review" };
  const calls: unknown[] = [];
  const desktop = tauriBackend(async (name, args) => { calls.push([name, args]); return expected; });
  assert.deepEqual(await desktop.changes("run-1", "card-a"), expected);
  assert.deepEqual(calls, [["run_changes", { run: "run-1", card: "card-a" }]]);
  const requests: string[] = [];
  const previous = globalThis.fetch;
  globalThis.fetch = async (input) => { requests.push(String(input)); return new Response(JSON.stringify(expected)); };
  try {
    assert.deepEqual(await devBackend.changes("run-1", "card-a"), expected);
    assert.equal(requests[0], "/api/changes?run=run-1&card=card-a");
  } finally { globalThis.fetch = previous; }
});

test("development adapter reports a rejected changes request", async () => {
  const previous = globalThis.fetch;
  globalThis.fetch = async () => new Response(JSON.stringify({ error: "작업 폴더를 찾지 못했습니다" }), { status: 400 });
  try { await assert.rejects(devBackend.changes("run-1", "card-a"), /작업 폴더/); }
  finally { globalThis.fetch = previous; }
});

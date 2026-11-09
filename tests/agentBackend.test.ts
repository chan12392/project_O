import assert from "node:assert/strict";
import { test } from "node:test";
import { demoBackend } from "../app/src/platform/backend/demo.ts";
import { devBackend } from "../app/src/platform/backend/dev.ts";

test("development agent status preserves detected variable names for each profile", async () => {
  const original = globalThis.fetch;
  const calls: string[] = [];
  globalThis.fetch = async (input) => {
    calls.push(String(input));
    return new Response(JSON.stringify([{ id: "claude", apiKeyEnv: ["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN"] }]));
  };
  try {
    for (const clean of [true, false]) {
      const [agent] = await devBackend.agents(clean);
      assert.deepEqual(agent.apiKeyEnv, ["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN"]);
    }
    assert.deepEqual(calls, ["/api/agents?clean=1", "/api/agents?clean=0"]);
  } finally { globalThis.fetch = original; }
});

test("demo agent statuses contain no detected API variables", async () => {
  for (const agent of await demoBackend.agents(false)) assert.deepEqual(agent.apiKeyEnv, []);
});

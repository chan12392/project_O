import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import type { Plan } from "../src/core/plan.ts";
import type { LaunchSpec } from "../src/core/providers.ts";
import { runPlan, type Spawner } from "../src/cli/runner.ts";
import { activeMarkers, createRun, readCooldowns, readEvents, startedOn } from "../src/cli/store.ts";

/** Stands in for a provider CLI: prints the given stream lines, then exits. */
function fakeCli(linesFor: (spec: LaunchSpec) => string[]): Spawner {
  return (spec, cwd) => {
    const script = `for (const l of ${JSON.stringify(linesFor(spec))}) console.log(l)`;
    return spawn(process.execPath, ["-e", script], { cwd, windowsHide: true, stdio: ["pipe", "pipe", "pipe"] });
  };
}

test("runs cards in parallel, records a limit as a provider cooldown", async () => {
  const home = mkdtempSync(join(tmpdir(), "o-test-"));
  const plan: Plan = {
    title: "테스트",
    project: tmpdir(),
    cards: [
      { id: "plan", provider: "claude", role: "설계", task: "설계해" },
      { id: "code", provider: "codex", role: "코딩", task: "구현해" },
    ],
  };
  createRun(home, "r1", plan);
  const resetsAt = Math.floor(Date.now() / 1000) + 3600;
  const status = await runPlan(
    home,
    "r1",
    plan,
    fakeCli((spec) =>
      spec.bin === "claude"
        ? [
            '{"type":"system","subtype":"init","session_id":"s1"}',
            '{"type":"assistant","message":{"content":[{"type":"text","text":"설계 끝"}]}}',
            '{"type":"result","subtype":"success","is_error":false,"result":"설계 끝"}',
          ]
        : [
            '{"type":"thread.started","thread_id":"t1"}',
            `{"type":"turn.failed","error":{"message":"usage limit, resets ${resetsAt}"}}`,
          ],
    ),
  );

  assert.equal(status, "failed");
  const events = readEvents(home, "r1");
  const ends = Object.fromEntries(events.filter((e) => e.t === "card.end").map((e) => [e.cardId, e.status]));
  assert.deepEqual(ends, { plan: "done", code: "failed" });
  assert.equal(events.at(-1)?.t, "run.end");
  assert.ok(events.some((e) => e.t === "card.limit" && e.cardId === "code"));
  assert.ok(Date.parse(readCooldowns(home).codex ?? "") > Date.now());
  assert.equal(startedOn(home, new Date()), 2);
  assert.deepEqual(activeMarkers(home), []);
});

test("a card whose provider is cooling down ends without launching", async () => {
  const home = mkdtempSync(join(tmpdir(), "o-test-"));
  const plan: Plan = { title: "t", project: tmpdir(), cards: [{ id: "a", provider: "codex", role: "r", task: "x" }] };
  createRun(home, "r2", plan);
  const { setCooldown } = await import("../src/cli/store.ts");
  setCooldown(home, "codex", new Date(Date.now() + 60_000).toISOString(), new Date());
  let launched = false;
  await runPlan(home, "r2", plan, (spec, cwd) => {
    launched = true;
    return fakeCli(() => [])(spec, cwd);
  });
  assert.equal(launched, false);
  assert.equal(startedOn(home, new Date()), 0);
});

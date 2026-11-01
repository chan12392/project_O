import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import type { Plan } from "../src/core/plan.ts";
import type { LaunchSpec } from "../src/core/providers.ts";
import { LIMITS } from "../src/core/policy.ts";
import { runPlan, type Spawner } from "../src/cli/runner.ts";
import { activeMarkers, clearActive, createRun, markActive, readCooldowns, readEvents, recordStart, requestStop, setCooldown, startedOn } from "../src/cli/store.ts";

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
            '{"type":"turn.failed","error":{"message":"usage limit reached"}}',
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

  setCooldown(home, "codex", new Date(Date.now() + 60_000).toISOString(), new Date());
  let launched = false;
  await runPlan(home, "r2", plan, (spec, cwd) => {
    launched = true;
    return fakeCli(() => [])(spec, cwd);
  });
  assert.equal(launched, false);
  assert.equal(startedOn(home, new Date()), 0);
});

const quiet = () => fakeCli(() => ['{"type":"result","subtype":"success","is_error":false,"result":"ok"}']);
const endsOf = (home: string, runId: string) => Object.fromEntries(readEvents(home, runId).flatMap((e) => (e.t === "card.end" ? [[e.cardId, e.status]] : [])));

test("the day's cap is checked again when a card starts, not only at dispatch", async () => {
  const home = mkdtempSync(join(tmpdir(), "o-test-"));
  for (let i = 0; i < LIMITS.maxCardsPerDay; i += 1) recordStart(home, "earlier", `c${i}`, "codex", new Date());
  const plan: Plan = { title: "t", project: tmpdir(), cards: [{ id: "a", provider: "claude", task: "x" }] };
  createRun(home, "r3", plan);
  assert.equal(await runPlan(home, "r3", plan, quiet()), "failed");
  assert.deepEqual(endsOf(home, "r3"), { a: "failed" });
  assert.equal(startedOn(home, new Date()), LIMITS.maxCardsPerDay);
});

test("a stop request ends the cards still waiting", async () => {
  const home = mkdtempSync(join(tmpdir(), "o-test-"));
  const plan: Plan = { title: "t", project: tmpdir(), cards: [{ id: "a", provider: "codex", task: "x" }] };
  createRun(home, "r4", plan);
  requestStop(home, "r4");
  assert.equal(await runPlan(home, "r4", plan, quiet()), "stopped");
  assert.deepEqual(endsOf(home, "r4"), { a: "stopped" });
});

test("a card waits in the queue while its provider is full, then runs", async () => {
  const home = mkdtempSync(join(tmpdir(), "o-test-"));
  // Live markers from another run fill agy's slots (this test process stands in as their pid).
  for (let i = 0; i < LIMITS.maxActivePerProvider.agy; i += 1) markActive(home, { runId: "other", cardId: `b${i}`, provider: "agy", pid: process.pid });
  const plan: Plan = { title: "t", project: tmpdir(), cards: [{ id: "a", provider: "agy", task: "x" }] };
  createRun(home, "r5", plan);
  setTimeout(() => {
    for (let i = 0; i < LIMITS.maxActivePerProvider.agy; i += 1) clearActive(home, "other", `b${i}`);
  }, 300);
  await runPlan(home, "r5", plan, fakeCli(() => ['{"event":"result","result":{"status":"SUCCESS","response":"ok"}}']));
  const kinds = readEvents(home, "r5").map((e) => e.t);
  assert.ok(kinds.indexOf("card.queued") < kinds.indexOf("card.start"));
  assert.deepEqual(endsOf(home, "r5"), { a: "done" });
});

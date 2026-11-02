import assert from "node:assert/strict";
import { test } from "node:test";

import { createAgentStore } from "../app/src/data/agents.ts";
import type { AgentStatus } from "../app/src/platform/backend/index.ts";

const claude: AgentStatus = { id: "claude", name: "Claude Code", installed: true, loggedIn: true, installHint: "", canInstall: true, installUrl: "" };
const codex: AgentStatus = { ...claude, id: "codex", name: "Codex" };

/** A load the test answers by hand, counting how often it was asked. */
function manual() {
  const asked: { clean: boolean; resolve: (list: AgentStatus[]) => void; reject: (e: Error) => void }[] = [];
  const load = (clean: boolean) => new Promise<AgentStatus[]>((resolve, reject) => asked.push({ clean, resolve, reject }));
  return { asked, load };
}

const tick = () => new Promise((r) => setTimeout(r, 0));

test("every screen shares one agent check per profile; it runs once until asked again", async () => {
  const { asked, load } = manual();
  const store = createAgentStore(load);
  // Onboarding, Settings → 에이전트 and the composer all look at the same profile.
  const off = [store.subscribe(false, () => {}), store.subscribe(false, () => {}), store.subscribe(false, () => {})];
  assert.equal(asked.length, 1);
  assert.equal(store.get(false).checking, true);
  asked[0].resolve([claude]);
  await tick();
  assert.deepEqual(store.get(false).agents, [claude]);
  assert.equal(store.get(false).checking, false);
  // A screen opened later reuses the answer.
  store.subscribe(false, () => {});
  assert.equal(asked.length, 1);
  // The other profile is its own check.
  store.subscribe(true, () => {});
  assert.equal(asked.length, 2);
  assert.equal(asked[1].clean, true);
  // "다시 확인" asks again; a failure keeps the last answer and says why.
  void store.check(false);
  asked[2].reject(new Error("o agents status 실패"));
  await tick();
  assert.deepEqual(store.get(false).agents, [claude]);
  assert.equal(store.get(false).error, "o agents status 실패");
  for (const stop of off) stop();
});

test("the last known agents are there at once, before this launch's check answers; an older answer never wins", async () => {
  const saved = new Map<string, string>();
  const storage = { get: (k: string) => saved.get(k) ?? null, set: (k: string, v: string) => void saved.set(k, v) };
  const first = manual();
  const before = createAgentStore(first.load, storage);
  before.subscribe(false, () => {});
  first.asked[0].resolve([claude, codex]);
  await tick();

  // The next launch: the composer can pick a lead right away while the 8–10 s check runs.
  const next = manual();
  const store = createAgentStore(next.load, storage);
  let heard = 0;
  store.subscribe(false, () => heard++);
  assert.deepEqual(store.get(false).agents, [claude, codex]);
  assert.equal(store.get(false).remembered, true);
  assert.equal(store.get(false).checking, true);
  void store.check(false);
  next.asked[1].resolve([claude]);
  next.asked[0].resolve([claude, codex]);
  await tick();
  assert.deepEqual(store.get(false).agents, [claude], "the older check overwrote the newer one");
  assert.equal(store.get(false).remembered, false);
  assert.ok(heard > 0);
});

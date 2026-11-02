import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import type { OEvent } from "../src/core/events.ts";
import { chipSummary, importCandidates, isRecent, listedRuns, ownerOf, sameFolder, threadStartedAt, within } from "../src/core/projects.ts";
import { snapshot, type RunView } from "../src/core/snapshot.ts";

const demo = JSON.parse(readFileSync(new URL("../app/src/data/demo-runs.json", import.meta.url), "utf8")) as { runs: { events: OEvent[] }[] };
const runs = demo.runs.map((r) => snapshot(r.events)).filter((r): r is RunView => Boolean(r));
const byId = (id: string) => runs.find((r) => r.runId === id)!;
const after = (iso: string, ms: number) => new Date(Date.parse(iso) + ms);

const ROOT = "C:/Users/me/.project_o/project";
const listed = [
  { path: `${ROOT}/blog`, name: "blog", kind: "root" as const },
  { path: `${ROOT}/test/probe`, name: "probe", kind: "test" as const },
  { path: "C:/work/app", name: "app", kind: "registered" as const },
  { path: "C:/work/app/packages/web", name: "web", kind: "registered" as const },
];

test("a folder belongs to itself or the deepest listed folder holding it, in any spelling", () => {
  assert.ok(sameFolder("C:\\Work\\App\\", "c:/work/app"));
  assert.ok(!sameFolder("/home/me/App", "/home/me/app"), "a POSIX path keeps its case");
  assert.ok(within("C:/work/app/src", "C:/work/app"));
  assert.ok(!within("C:/work/apple", "C:/work/app"), "a sibling with the same prefix is not inside");
  assert.equal(ownerOf(listed, "C:\\work\\app\\src")?.name, "app");
  assert.equal(ownerOf(listed, "C:/work/app/packages/web/src")?.name, "web");
  assert.equal(ownerOf(listed, `${ROOT}/test/probe/sub`)?.name, "probe");
  assert.equal(ownerOf(listed, `${ROOT}/test`), undefined, "the test folder itself is no project");
  assert.equal(ownerOf(listed, "C:/Users/me"), undefined);
});

test("runs outside every listed project are left out of the sidebar and its counts", () => {
  const sample = [
    { runId: "a", project: "C:/work/app" },
    { runId: "b", project: "C:\\work\\app\\src" },
    { runId: "c", project: "C:/Users/me" },
    { runId: "session-d", project: "C:/Users/me/AppData/Local/Temp/x" },
    { runId: "e", project: `${ROOT}/blog` },
  ];
  assert.deepEqual(listedRuns(listed, sample).map((r) => r.runId), ["a", "b", "e"]);
  assert.deepEqual(listedRuns([], sample), []);
});

test("history offers each new folder once, newest first, without temp folders or listed ones", () => {
  const history = [
    "C:/work/shop",
    "C:\\work\\shop\\",
    "C:/Users/me/AppData/Local/Temp/o-test-1",
    "/tmp/scratch",
    "C:/work/app/src",
    `${ROOT}/new-one`,
    "relative/path",
    "",
    "D:/games/mod",
  ];
  assert.deepEqual(importCandidates(history, listed, ROOT), ["C:/work/shop", "D:/games/mod"]);
});

test("a run's chips sum up per agent and per state, and cards needing a look stay one by one", () => {
  const team = byId("20261101-145332-q3eg");
  const summary = chipSummary(team, after(team.endedAt!, 60_000));
  const total = team.cards.length;
  assert.equal(summary.providers.reduce((n, p) => n + p.count, 0), total);
  assert.equal(new Set(summary.providers.map((p) => p.provider)).size, summary.providers.length, "one entry per agent");
  assert.equal(summary.states.reduce((n, s) => n + s.count, 0), total);
  // The card that asked the lead and got no answer, and the one that failed, are shown on their own.
  assert.deepEqual(summary.attention.map((a) => [a.card.id, a.why]), [["a", "answer"], ["c", "failed"]]);

  const failed = byId("20261101-100916-402x");
  const failedSummary = chipSummary(failed, after(failed.endedAt ?? failed.startedAt, 60_000));
  assert.ok(failedSummary.attention.some((a) => a.why === "failed"));
  assert.ok(failedSummary.states.some((s) => s.state === "failed"));
  assert.deepEqual(
    failedSummary.states.map((s) => s.state),
    ["done", "running", "waiting", "failed", "stopped"].filter((s) => failedSummary.states.some((x) => x.state === s)),
    "states keep a fixed reading order",
  );
});

test("four cards of one agent read as one chip", () => {
  const run = byId("20261101-162328-9p0b");
  const four: RunView = { ...run, cards: [0, 1, 2, 3].map((i) => ({ ...run.cards[0], id: `c${i}`, provider: "codex", state: "done" as const })) };
  const summary = chipSummary(four, after(run.endedAt!, 60_000));
  assert.deepEqual(summary.providers, [{ provider: "codex", count: 4 }]);
  assert.deepEqual(summary.states, [{ state: "done", count: 4 }]);
  assert.deepEqual(summary.attention, []);
});

test("lead thread ids carry their start time; old finished runs are not recent", () => {
  assert.equal(threadStartedAt("20261102-091500-ab12"), "2026-11-02T09:15:00.000Z");
  assert.equal(threadStartedAt("not-a-thread"), undefined);
  const now = new Date("2026-11-20T00:00:00.000Z");
  assert.ok(isRecent({ status: "running", startedAt: "2026-01-01T00:00:00.000Z" }, now));
  assert.ok(isRecent({ status: "done", startedAt: "2026-11-12T00:00:00.000Z", endedAt: "2026-11-13T01:00:00.000Z" }, now));
  assert.ok(!isRecent({ status: "done", startedAt: "2026-11-01T00:00:00.000Z", endedAt: "2026-11-12T23:00:00.000Z" }, now));
});

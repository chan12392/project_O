import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import type { OEvent } from "../src/core/events.ts";
import { describeTool, snapshot } from "../src/core/snapshot.ts";

// The app's demo fixture is today's real end-to-end run, paths sanitised.
const fixture = JSON.parse(readFileSync(new URL("../app/src/data/demo-run.json", import.meta.url), "utf8")) as { events: OEvent[] };

test("folds a real run into per-card state, activity and log", () => {
  const run = snapshot(fixture.events);
  assert.ok(run);
  assert.equal(run.status, "failed");
  assert.deepEqual(run.cards.map((c) => [c.id, c.state]), [["code", "done"], ["design", "failed"], ["research", "done"]]);
  const design = run.cards.find((c) => c.id === "design");
  assert.match(design?.activity ?? "", /RunCommand/);
  assert.ok(run.cards.every((c) => c.startedAt && c.endedAt));
  assert.ok((run.cards.find((c) => c.id === "code")?.log.length ?? 0) > 20);
});

test("a prefix of the stream shows cards still running", () => {
  const firstStarts = fixture.events.findIndex((e) => e.t === "card.session");
  const run = snapshot(fixture.events.slice(0, firstStarts + 1));
  assert.equal(run?.status, "running");
  assert.ok(run?.cards.every((c) => c.state === "running"));
});

test("tool activity reads as an act, the log keeps the raw event", () => {
  assert.equal(describeTool("shell", 'ego-browser nodejs -e \\"const task\\"'), '명령 실행: ego-browser nodejs -e "const task"');
  assert.equal(describeTool("read_file", "src/app.js"), "파일 읽기: src/app.js");
  assert.equal(describeTool("Edit", ""), "파일 수정");
  assert.equal(describeTool("todo_write", "x"), "todo_write: x");
  const run = snapshot(fixture.events);
  const design = run?.cards.find((c) => c.id === "design");
  assert.ok(design?.log.some((l) => l.kind === "tool" && l.text.startsWith("run_command")));
});

test("no run.start means nothing to show", () => {
  assert.equal(snapshot([]), undefined);
});

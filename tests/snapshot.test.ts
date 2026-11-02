import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import type { OEvent } from "../src/core/events.ts";
import { describeTool, snapshot } from "../src/core/snapshot.ts";

// The app's demo fixture is today's real end-to-end run, paths sanitised.
const demo = JSON.parse(readFileSync(new URL("../app/src/data/demo-runs.json", import.meta.url), "utf8")) as { runs: { events: OEvent[] }[] };
const byId = (runId: string) => {
  const run = demo.runs.find((r) => r.events[0].t === "run.start" && r.events[0].runId === runId);
  if (!run) throw new Error(`fixture lacks run ${runId}`);
  return run;
};
const fixture = byId("20261101-100916-402x");

test("real team runs carry their messages between codex, claude and agy", () => {
  const [agyCodex, codexClaude] = ["20261101-145813-t353", "20261101-145332-q3eg"].map((id) => snapshot(byId(id).events));
  assert.deepEqual(agyCodex?.messages.map((m) => `${m.from}>${m.to}`), ["c>d", "d>c"]);
  assert.deepEqual(codexClaude?.messages.map((m) => `${m.from}>${m.to}`), ["a>b", "b>a", "a>lead"]);
  assert.equal(codexClaude?.cards.find((c) => c.id === "a")?.label, "a");
  assert.equal(codexClaude?.cards.find((c) => c.id === "b")?.label, "답장 담당");
});

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
  assert.equal(describeTool("edit", '[{"path":"C:/p\\\\app\\\\align.py","kind":"add"},{"path":"src/app.js","kind":"update"}]'), "파일 수정: align.py 추가, app.js 수정");
  assert.equal(describeTool("edit", '[{"path":"C:/projects/todo-app/index.html","kind":"update"},{"path":"C:/projects/to'), "파일 수정: index.html 수정 외");
  assert.equal(describeTool("edit", '[{"path":"C:/projects/todo-app'), "파일 수정");
  assert.equal(describeTool("todo_write", "x"), "todo_write: x");
  const run = snapshot(fixture.events);
  const design = run?.cards.find((c) => c.id === "design");
  assert.ok(design?.log.some((l) => l.kind === "tool" && l.text.startsWith("run_command")));
});

test("team messages land on the run and in each party's log", () => {
  const start = fixture.events[0];
  const run = snapshot([
    start,
    { t: "msg", at: "2026-11-01T10:10:00.000Z", from: "code", to: "research", text: "app.js 끝났어요" },
    { t: "msg", at: "2026-11-01T10:11:00.000Z", from: "lead", to: "all", text: "모두 마무리해 주세요" },
  ]);
  assert.equal(run?.messages.length, 2);
  const log = (id: string) => run?.cards.find((c) => c.id === id)?.log.filter((l) => l.kind === "team").map((l) => l.text);
  assert.deepEqual(log("code"), ["app.js 끝났어요", "모두 마무리해 주세요"]);
  assert.deepEqual(log("research"), ["app.js 끝났어요", "모두 마무리해 주세요"]);
  assert.deepEqual(log("design"), ["모두 마무리해 주세요"]);
  assert.equal(run?.cards[0].label, "코딩");
});

test("a budget adjustment shows the effort the card ran with in its activity and log", () => {
  const run = snapshot([
    { t: "run.start", at: "2026-11-02T10:00:00.000Z", runId: "r", title: "t", project: "C:/p", cards: [{ id: "a", provider: "codex", label: "a", effort: "xhigh" }, { id: "b", provider: "codex", label: "b" }] },
    { t: "card.budget", at: "2026-11-02T10:00:01.000Z", cardId: "a", provider: "codex", effortFrom: "xhigh", effortTo: "medium", spent: 9.5, allowance: 13 },
    { t: "card.budget", at: "2026-11-02T10:00:01.000Z", cardId: "b", provider: "codex", effortTo: "low", spent: 9.5, allowance: 13 },
  ]);
  const [a, b] = run?.cards ?? [];
  assert.equal(a.effort, "medium");
  assert.equal(a.activity, "사용 예산: effort xhigh → medium (오늘 9.5/13%p)");
  assert.deepEqual(a.log.map((l) => l.kind), ["budget"]);
  assert.equal(b.effort, "low");
  assert.match(b.activity, /effort 기본 → low/);
});

// Same events as crates/o-core/tests/snapshot.rs `a_repeated_card_id_is_one_card_kept_by_the_first`.
test("a repeated card id is one card, kept by the first", () => {
  const run = snapshot([
    { t: "run.start", at: "2026-11-02T10:00:00.000Z", runId: "r", title: "t", project: "C:/p", cards: [{ id: "a1", provider: "claude", label: "첫" }, { id: "b2", provider: "claude", label: "b" }, { id: "a1", provider: "claude", label: "둘" }] },
    { t: "card.start", at: "2026-11-02T10:00:01.000Z", cardId: "a1", pid: undefined },
    { t: "card.end", at: "2026-11-02T10:00:02.000Z", cardId: "a1", status: "done", result: "결과", exitCode: null },
  ]);
  assert.deepEqual(run?.cards.map((c) => [c.id, c.label, c.state, c.result]), [["a1", "첫", "done", "결과"], ["b2", "b", "waiting", undefined]]);
});

test("no run.start means nothing to show", () => {
  assert.equal(snapshot([]), undefined);
});

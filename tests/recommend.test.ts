import assert from "node:assert/strict";
import { test } from "node:test";

import { isSessionRun, nextUp, nextUpByProject, recommend, waited } from "../src/core/attention.ts";
import type { CardView, RunView } from "../src/core/snapshot.ts";

const NOW = new Date("2026-11-10T12:00:00.000Z");

function card(id: string, state: CardView["state"], extra: Partial<CardView> = {}): CardView {
  return { id, provider: "claude", label: `${id} 카드`, state, activity: "", log: [], usage: { input: 0, cached: 0, output: 0 }, ...extra };
}

function run(runId: string, project: string, startedAt: string, cards: CardView[], extra: Partial<RunView> = {}): RunView {
  return { runId, title: runId, project, startedAt, status: "done", cards, messages: [], quotas: [], ...extra };
}

/** Five projects: failed and stopped work, a question, a PR, one left idle with a card never run, one idle but finished. */
function fleet(): RunView[] {
  return [
    run("alpha-1", "C:/p/alpha", "2026-11-07T10:00:00.000Z", [card("f1", "failed", { endedAt: "2026-11-07T12:00:00.000Z", result: "빌드 실패" }), card("s1", "stopped", { endedAt: "2026-11-10T10:00:00.000Z" })], {
      status: "failed",
      endedAt: "2026-11-10T10:00:00.000Z",
    }),
    run("beta-1", "C:/p/beta", "2026-11-10T10:30:00.000Z", [card("q", "running", { startedAt: "2026-11-10T10:30:00.000Z", log: [{ at: "2026-11-10T11:55:00.000Z", kind: "tool", text: "Read x" }] })], {
      status: "running",
      endedAt: undefined,
      messages: [{ at: "2026-11-10T11:00:00.000Z", from: "q", to: "lead", text: "어느 쪽으로 할까요?" }],
    }),
    run("gamma-1", "C:/p/gamma", "2026-11-09T10:00:00.000Z", [card("pr", "done", { endedAt: "2026-11-09T12:00:00.000Z", result: "https://github.com/o/r/pull/7 올림" })], {
      endedAt: "2026-11-09T12:00:00.000Z",
    }),
    run("delta-1", "C:\\p\\delta", "2026-11-01T10:00:00.000Z", [card("d1", "done", { endedAt: "2026-11-02T09:00:00.000Z" }), card("w1", "waiting")], {
      status: "stopped",
      endedAt: "2026-11-02T10:00:00.000Z",
    }),
    run("eps-1", "C:/p/eps", "2026-11-04T10:00:00.000Z", [card("e1", "done", { endedAt: "2026-11-05T10:00:00.000Z" })], { endedAt: "2026-11-05T10:00:00.000Z" }),
  ];
}

test("recommendations: stopped work oldest first, then questions, PRs and idle projects, each with a reason", () => {
  const picks = recommend(fleet(), NOW);
  assert.deepEqual(
    picks.map((p) => [p.kind, p.project, p.title, p.reason]),
    [
      ["failed", "C:/p/alpha", "f1 카드", "3일째 멈춤 · 실패"],
      ["stopped", "C:/p/alpha", "s1 카드", "2시간째 멈춤 · 중지됨"],
      ["answer", "C:/p/beta", "q 카드", "1시간째 답을 기다림"],
      ["review", "C:/p/gamma", "pr 카드", "PR이 1일째 검토를 기다림"],
      ["idle", "C:\\p\\delta", "delta", "8일째 손대지 않음 · 끝나지 않은 카드 1장"],
    ],
  );
  // Tickets behind the picks are the Next up items themselves.
  assert.equal(picks[0].item?.key, nextUp(fleet(), NOW).stopped.find((i) => i.cardId === "f1")?.key);
  assert.equal(picks[4].item, undefined);
});

test("recommendations honour ticked items, the idle threshold and the limit", () => {
  const first = recommend(fleet(), NOW)[0];
  const handled = new Set([first.key]);
  assert.ok(!recommend(fleet(), NOW, handled).some((p) => p.key === first.key));
  assert.ok(!recommend(fleet(), NOW, new Set(), { idleDays: 10 }).some((p) => p.kind === "idle"));
  assert.deepEqual(recommend(fleet(), NOW, new Set(), { limit: 2 }).map((p) => p.kind), ["failed", "stopped"]);
  // An idle project can be ticked off as well.
  const idle = recommend(fleet(), NOW).find((p) => p.kind === "idle")!;
  assert.ok(!recommend(fleet(), NOW, new Set([idle.key])).some((p) => p.kind === "idle"));
});

test("cards reporting the same PR take one recommendation, with every card still reachable", () => {
  const pr = "https://github.com/o/r/pull/27";
  const reviewer = (id: string, endedAt: string) => card(id, "done", { endedAt, result: `${pr} 올림` });
  const runs = [
    run("s-1", "C:/p/graf", "2026-11-06T10:00:00.000Z", [reviewer("cli", "2026-11-07T12:00:00.000Z"), reviewer("core", "2026-11-07T11:00:00.000Z")], { endedAt: "2026-11-07T12:00:00.000Z" }),
    run("s-2", "C:/p/graf", "2026-11-06T10:00:00.000Z", [reviewer("ask", "2026-11-07T13:00:00.000Z"), reviewer("tool", "2026-11-07T14:00:00.000Z")], { endedAt: "2026-11-07T14:00:00.000Z" }),
    run("s-3", "C:/p/graf", "2026-11-09T10:00:00.000Z", [card("other", "done", { endedAt: "2026-11-09T12:00:00.000Z", result: "https://github.com/o/r/pull/28" })], { endedAt: "2026-11-09T12:00:00.000Z" }),
  ];
  const picks = recommend(runs, NOW);
  assert.deepEqual(
    picks.map((p) => [p.title, p.reason, p.item?.pr]),
    [
      ["core 카드", "PR이 3일째 검토를 기다림 · 카드 4장", pr],
      ["other 카드", "PR이 1일째 검토를 기다림", "https://github.com/o/r/pull/28"],
    ],
  );
  // The oldest card leads; the others stay reachable from the same pick, oldest first.
  assert.deepEqual(picks[0].samePr?.map((i) => i.cardId), ["cli", "ask", "tool"]);
  assert.equal(picks[1].samePr, undefined);
  // The project's own list still has a ticket per card.
  assert.equal(nextUp(runs, NOW).todo.filter((i) => i.pr === pr).length, 4);
});

test("a project with a ticket already is not also called idle; a finished idle project is left alone", () => {
  const old = fleet().map((r) => (r.runId === "alpha-1" ? { ...r, cards: [r.cards[0]], endedAt: "2026-11-07T12:00:00.000Z" } : r));
  const picks = recommend(old, NOW);
  assert.equal(picks.filter((p) => p.project === "C:/p/alpha").length, 1);
  assert.ok(!picks.some((p) => p.project === "C:/p/eps"));
});

test("Claude Code sessions: no result check for the owner's own session, but a failed subagent is stopped work", () => {
  const session = run("session-abc", "C:/p/solo", "2026-11-10T09:00:00.000Z", [card("a1", "done", { endedAt: "2026-11-10T09:30:00.000Z" }), card("a2", "failed", { endedAt: "2026-11-10T09:40:00.000Z" })], {
    status: "failed",
    endedAt: "2026-11-10T11:00:00.000Z",
  });
  assert.ok(isSessionRun(session));
  const finished = { ...session, status: "done" as const, cards: [session.cards[0]] };
  assert.equal(nextUp([finished], NOW).todo.length, 0);
  assert.deepEqual(
    recommend([session], NOW).map((p) => [p.kind, p.reason]),
    [["failed", "2시간째 멈춤 · 실패"]],
  );
});

test("next up across projects: most urgent project first, items kept per project", () => {
  const groups = nextUpByProject(fleet(), NOW);
  assert.deepEqual(
    groups.map((g) => [g.project, g.todo.map((i) => i.kind), g.stopped.map((i) => i.kind)]),
    [
      ["C:/p/alpha", [], ["failed", "stopped"]],
      ["C:/p/beta", ["answer"], []],
      ["C:/p/gamma", ["review"], []],
    ],
  );
  // Same urgency: the project that changed last comes first.
  const twins = [
    run("x-1", "C:/p/x", "2026-11-10T08:00:00.000Z", [card("x", "failed", { endedAt: "2026-11-10T08:00:00.000Z" })], { status: "failed" }),
    run("y-1", "C:/p/y", "2026-11-10T09:00:00.000Z", [card("y", "failed", { endedAt: "2026-11-10T09:00:00.000Z" })], { status: "failed" }),
  ];
  assert.deepEqual(nextUpByProject(twins, NOW).map((g) => g.project), ["C:/p/y", "C:/p/x"]);
});

test("waited reads like a person says it", () => {
  const ago = (ms: number) => new Date(NOW.getTime() - ms).toISOString();
  assert.equal(waited(ago(20_000), NOW), "방금");
  assert.equal(waited(ago(5 * 60_000), NOW), "5분째");
  assert.equal(waited(ago(3 * 3_600_000), NOW), "3시간째");
  assert.equal(waited(ago(3 * 86_400_000), NOW), "3일째");
});

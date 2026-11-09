import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { SILENT_MS, activity, fleetStats, isNote, needsAction, nextUp, recommend, ticketCommand } from "../src/core/attention.ts";
import type { OEvent } from "../src/core/events.ts";
import { snapshot, type RunView } from "../src/core/snapshot.ts";

const demo = JSON.parse(readFileSync(new URL("../app/src/data/demo-runs.json", import.meta.url), "utf8")) as { runs: { events: OEvent[] }[] };
const runs = demo.runs.map((r) => snapshot(r.events)).filter((r): r is RunView => Boolean(r));
const byId = (id: string) => runs.find((r) => r.runId === id)!;
const after = (iso: string, ms: number) => new Date(Date.parse(iso) + ms);

test("a card writing to the lead waits for an answer until the lead writes back", () => {
  const team = byId("20261101-145332-q3eg");
  const { todo } = nextUp([team], after(team.endedAt!, 60_000));
  const answer = todo.filter((i) => i.kind === "answer");
  assert.equal(answer.length, 1);
  assert.equal(answer[0].cardId, "a");
  // Answered once the lead speaks after it.
  const replied: RunView = { ...team, messages: [...team.messages, { at: after(answer[0].since, 1000).toISOString(), from: "lead", to: "a", text: "확인" }] };
  assert.equal(nextUp([replied], after(team.endedAt!, 60_000)).todo.filter((i) => i.kind === "answer").length, 0);
});

test("failed and stopped cards land on the stopped list, a finished run asks to be checked", () => {
  const todoApp = byId("20261101-100916-402x");
  const { todo, stopped } = nextUp([todoApp, byId("20261101-162328-9p0b")], after(byId("20261101-162328-9p0b").endedAt!, 60_000));
  assert.deepEqual(stopped.map((i) => [i.kind, i.cardId]), [["failed", "design"]]);
  assert.ok(todo.some((i) => i.kind === "check" && i.runId === "20261101-162328-9p0b"));
  // A failed run is not a finished one to check.
  assert.ok(!todo.some((i) => i.kind === "check" && i.runId === todoApp.runId));
});

test("a run blocked before any card appears in stopped work and the dashboard", () => {
  const events: OEvent[] = [
    { t: "run.start", at: "2026-11-02T10:00:00Z", runId: "blocked", title: "작업", project: "C:/p", cards: [] },
    { t: "run.blocked", at: "2026-11-02T10:00:01Z", runId: "blocked", violations: [{ rule: "budget", message: "오늘 예산 소진" }] },
    { t: "run.end", at: "2026-11-02T10:00:02Z", runId: "blocked", status: "failed" },
  ];
  const run = snapshot(events)!;
  const now = new Date("2026-11-02T10:01:00Z");
  assert.equal(nextUp([run], now).stopped[0].detail, "오늘 예산 소진");
  assert.equal(fleetStats([run], now).stopped, 1);
  assert.match(activity([run], 10)[0].detail, /오늘 예산 소진/);
});

test("a running card that goes quiet is flagged; a ticked item stays gone", () => {
  const run = byId("20261101-162328-9p0b");
  const live: RunView = { ...run, status: "running", endedAt: undefined, cards: run.cards.map((c) => ({ ...c, state: "running", endedAt: undefined })) };
  const last = live.cards[0].log.at(-1)!.at;
  assert.equal(nextUp([live], after(last, SILENT_MS - 1000)).stopped.length, 0);
  const quiet = nextUp([live], after(last, SILENT_MS + 1000)).stopped;
  assert.deepEqual(quiet.map((i) => i.kind), ["silent"]);
  assert.equal(nextUp([live], after(last, SILENT_MS + 1000), new Set([quiet[0].key])).stopped.length, 0);

  // A message someone else sent it is not a sign of life.
  const card = live.cards[0];
  const heard = { ...card, log: [...card.log, { at: after(last, SILENT_MS).toISOString(), kind: "team" as const, text: "hi", team: { from: "other", to: card.id } }] };
  assert.deepEqual(nextUp([{ ...live, cards: [heard] }], after(last, SILENT_MS + 1000)).stopped.map((i) => i.kind), ["silent"]);
});

test("the lead answering one card leaves the other card's question waiting", () => {
  const team = byId("20261101-145332-q3eg");
  const at = (ms: number) => after(team.endedAt!, ms).toISOString();
  const asked: RunView = {
    ...team,
    messages: [
      { at: at(1000), from: "a", to: "lead", text: "어느 쪽으로 할까요?" },
      { at: at(2000), from: "b", to: "lead", text: "확인 부탁드립니다" },
      { at: at(3000), from: "lead", to: "b", text: "확인" },
    ],
  };
  assert.deepEqual(nextUp([asked], after(team.endedAt!, 60_000)).todo.filter((i) => i.kind === "answer").map((i) => i.cardId), ["a"]);
});

test("badges count what waits on a person: not a note to read, not a Claude Code session's ticket", () => {
  const team = byId("20261101-145332-q3eg");
  const at = (ms: number) => after(team.endedAt!, ms).toISOString();
  const told: RunView = {
    ...team,
    messages: [
      { at: at(1000), from: "a", to: "lead", text: "어느 쪽으로 할까요?" },
      { at: at(2000), from: "b", to: "lead", text: "README 정리 끝냈습니다." },
    ],
  };
  const { todo } = nextUp([told], after(team.endedAt!, 60_000));
  const answers = todo.filter((i) => i.kind === "answer");
  assert.deepEqual(answers.map((i) => [i.cardId, isNote(i)]), [["a", false], ["b", true]]);
  assert.deepEqual(answers.filter(needsAction).map((i) => i.cardId), ["a"]);
  // The same tickets from a Claude Code session are read-only records.
  const session = { ...told, runId: "session-0f3c" };
  assert.equal(nextUp([session], after(team.endedAt!, 60_000)).todo.filter(needsAction).length, 0);
  // A note is no "답을 기다림" recommendation either.
  assert.deepEqual(recommend([told], after(team.endedAt!, 60_000)).filter((p) => p.kind === "answer").map((p) => p.item?.cardId), ["a"]);
});

test("explicit message kind takes precedence over punctuation while older messages keep the heuristic", () => {
  const team = byId("20261101-145332-q3eg");
  const at = (ms: number) => after(team.endedAt!, ms).toISOString();
  const told: RunView = { ...team, messages: [
    { at: at(1000), from: "a", to: "lead", kind: "question", text: "방향을 정해야 합니다" },
    { at: at(2000), from: "b", to: "lead", kind: "message", text: "질문 예시: 어떻게 할까요?" },
  ] };
  const answers = nextUp([told], after(team.endedAt!, 60_000)).todo.filter((i) => i.kind === "answer");
  assert.deepEqual(answers.map((i) => [i.cardId, isNote(i)]), [["a", false], ["b", true]]);
});

test("only a reply linked to its question clears it, and the copied command preserves the link", () => {
  const team = byId("20261101-145332-q3eg");
  const at = (ms: number) => after(team.endedAt!, ms).toISOString();
  const asked: RunView = { ...team, messages: [
    { at: at(1000), id: "q1", kind: "question", from: "a", to: "lead", text: "첫 질문" },
    { at: at(2000), id: "q2", kind: "question", from: "a", to: "lead", text: "둘째 질문" },
    { at: at(3000), from: "lead", to: "all", text: "일반 공지" },
    { at: at(4000), from: "lead", to: "a", replyTo: "q1", text: "첫 답변" },
  ] };
  const answers = nextUp([asked], after(team.endedAt!, 60_000)).todo.filter((i) => i.kind === "answer");
  assert.deepEqual(answers.map((i) => i.questionId), ["q2"]);
  assert.match(ticketCommand(answers[0])!.text, /--reply-to q2/);
  assert.equal(ticketCommand({ ...answers[0], questionId: "q;bad" }), undefined);
});

test("an approval request remains visible until the matching direct reply", () => {
  const team = byId("20261101-145332-q3eg");
  const at = (ms: number) => after(team.endedAt!, ms).toISOString();
  const asked: RunView = { ...team, messages: [
    { at: at(1000), id: "approval1", kind: "approval", from: "a", to: "lead", text: "계속해도 되나요?" },
    { at: at(2000), from: "lead", to: "all", text: "일반 공지" },
  ] };
  const items = nextUp([asked], after(team.endedAt!, 60_000)).todo.filter((item) => item.kind === "answer");
  assert.equal(items.length, 1);
  assert.equal(items[0].questionId, "approval1");
  assert.match(items[0].detail ?? "", /승인 요청/);
  assert.equal(needsAction(items[0]), true);
  const replied: RunView = { ...asked, messages: [...asked.messages, { at: at(3000), from: "lead", to: "a", replyTo: "approval1", text: "확인" }] };
  assert.equal(nextUp([replied], after(team.endedAt!, 60_000)).todo.filter((item) => item.kind === "answer").length, 0);
});

test("a question or approval the card cancelled leaves next up, picks and the action count", () => {
  const team = byId("20261101-145332-q3eg");
  const at = (ms: number) => after(team.endedAt!, ms).toISOString();
  const now = after(team.endedAt!, 60_000);
  for (const kind of ["question", "approval"] as const) {
    const asked: RunView = { ...team, status: "running", messages: [
      { at: at(1000), id: "gone", kind, from: "a", to: "lead", text: "취소할 질문인가요?", recipients: { lead: "cancelled" } },
      { at: at(2000), id: "open", kind, from: "a", to: "lead", text: "남은 질문인가요?", recipients: { lead: "stored" } },
    ] };
    const answers = nextUp([asked], now).todo.filter((i) => i.kind === "answer");
    assert.deepEqual(answers.map((i) => i.questionId), ["open"], kind);
    assert.equal(answers.filter(needsAction).length, 1, kind);
    assert.deepEqual(recommend([asked], now).filter((s) => s.kind === "answer").map((s) => s.item?.questionId), ["open"], kind);
  }
});

test("fleet stats and activity read the same runs", () => {
  const now = after(byId("20261101-162328-9p0b").endedAt!, 0);
  const stats = fleetStats(runs, now);
  assert.equal(stats.running, 0);
  assert.equal(stats.stopped, 2);
  assert.equal(stats.prs, 0);
  const feed = activity(runs, 5);
  assert.equal(feed.length, 5);
  assert.ok(feed.every((item, i) => i === 0 || feed[i - 1].at >= item.at));
});

test("tickets offer a command only for what the app cannot do yet, and none for Claude Code sessions", () => {
  for (const kind of ["silent", "answer"] as const) {
    const run = ticketCommand({ kind, runId: "20261101-162328-9p0b", cardId: "a" });
    assert.ok(run, kind);
    assert.ok(run.text.includes("20261101-162328-9p0b"), kind);
    assert.equal(ticketCommand({ kind, runId: "session-0f3c", cardId: "a" }), undefined, kind);
  }
  assert.equal(ticketCommand({ kind: "silent", runId: "20261101-162328-9p0b", cardId: "a" })?.text, "o stop 20261101-162328-9p0b");
  // A status command would print what the record already shows.
  for (const kind of ["failed", "stopped", "limit", "review", "check", "queued"] as const) {
    assert.equal(ticketCommand({ kind, runId: "20261101-162328-9p0b", cardId: "a" }), undefined, kind);
  }
});

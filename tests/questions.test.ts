import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import type { OEvent } from "../src/core/events.ts";
import { leadItems } from "../src/core/lead.ts";
import { EMPTY_ANSWER, answerText, askNotice, cardQuestions, isComplete, leadRuns, parseCardQuestion, pendingLeadAsk, toggle } from "../src/core/questions.ts";
import { snapshot, type RunView } from "../src/core/snapshot.ts";

const demo = JSON.parse(readFileSync(new URL("../app/src/data/demo-runs.json", import.meta.url), "utf8")) as { runs: { events: OEvent[] }[] };
const team = demo.runs.map((r) => snapshot(r.events)).find((r) => r?.runId === "20261101-145332-q3eg")!;
const at = (ms: number) => new Date(Date.parse(team.startedAt) + ms).toISOString();
/** The team run still going, both cards at work. */
const live: RunView = { ...team, status: "running", endedAt: undefined, cards: team.cards.map((c) => ({ ...c, state: "running", endedAt: undefined })) };

const ask = {
  t: "lead.ask",
  at: "2026-11-04T10:00:01Z",
  askId: "th-q1",
  provider: "claude",
  intro: "먼저 확인할 것",
  questions: [
    { header: "멀티", question: "멀티가 무엇인가요?", options: [{ label: "한 PC" }, { label: "여러 PC" }, { label: "둘 다" }] },
    { question: "언어는?", options: [{ label: "Python" }, { label: "C#" }], multi: true },
  ],
};
const user = (at: string, text: string) => ({ t: "lead.user", at, text, provider: "claude", effort: "high" });

test("the lead's questions wait until a message follows them, and malformed ones are left out", () => {
  const items = leadItems([user("2026-11-04T10:00:00Z", "스킬 만들어 줘"), ask]);
  const pending = pendingLeadAsk(items)!;
  assert.equal(pending.key, "lead:th-q1");
  assert.equal(pending.intro, "먼저 확인할 것");
  assert.deepEqual(pending.questions.map((q) => [q.header, q.options.length, q.multi]), [["멀티", 3, false], [undefined, 2, true]]);
  // The picks (or anything the person writes instead) answer it.
  assert.equal(pendingLeadAsk(leadItems([ask, user("2026-11-04T10:01:00Z", "[질문 답변]")])), undefined);
  const malformed = { ...ask, questions: [{ question: "보기 하나", options: [{ label: "A" }] }, null] };
  assert.equal(leadItems([malformed]).length, 0);
});

test("every question needs a pick or the person's own words before it can go", () => {
  const { questions } = pendingLeadAsk(leadItems([ask]))!;
  const one = toggle(EMPTY_ANSWER, "한 PC", false);
  assert.equal(isComplete(questions, [one, EMPTY_ANSWER]), false);
  // One choice replaces the other; a multi question keeps several.
  assert.deepEqual(toggle(one, "둘 다", false).chosen, ["둘 다"]);
  const both = toggle(toggle(EMPTY_ANSWER, "Python", true), "C#", true);
  assert.deepEqual(both.chosen, ["Python", "C#"]);
  assert.equal(isComplete(questions, [one, both]), true);
  assert.equal(isComplete(questions, [one, { chosen: [], other: "Rust" }]), true);
  assert.equal(isComplete(questions, [one, { chosen: [], other: "  " }]), false);
});

test("the lead gets each question with its answer as the next message", () => {
  const pending = pendingLeadAsk(leadItems([ask]))!;
  const text = answerText(pending, [{ chosen: ["한 PC"], other: "" }, { chosen: ["Python"], other: "Rust도" }]);
  assert.equal(text, "[질문 답변]\n- 멀티: 한 PC\n- 언어는?: Python, Rust도");
  assert.deepEqual(askNotice(pending), { title: "총괄의 질문", body: "멀티가 무엇인가요? 외 1개" });
});

test("a card's choices follow \"[보기:\"; without two of them it is answered in words", () => {
  assert.deepEqual(parseCardQuestion("어느 쪽으로 할까요? [보기: 탭 | 분할 | 탭]"), { question: "어느 쪽으로 할까요?", options: [{ label: "탭" }, { label: "분할" }], multi: false });
  assert.deepEqual(parseCardQuestion("어느 쪽으로 할까요? [보기: 탭]").options, []);
  assert.equal(parseCardQuestion("API 키 위치를 알려 주세요").question, "API 키 위치를 알려 주세요");
});

test("a card's question waits while its run goes and the lead has not answered that card", () => {
  const asked: RunView = {
    ...live,
    messages: [
      { at: at(1000), from: "a", to: "lead", text: "어느 쪽으로 할까요? [보기: 탭 | 분할]" },
      { at: at(2000), from: "b", to: "lead", text: "README 정리 끝냈습니다." },
    ],
  };
  const [only, ...rest] = cardQuestions([asked]);
  assert.equal(rest.length, 0, "a report asks nothing");
  assert.equal(only.source.kind === "card" && only.source.cardId, "a");
  assert.equal(answerText(only, [{ chosen: ["탭"], other: "" }]), "답: 탭");
  // Answered by the lead, or the run over: nothing waits.
  assert.equal(cardQuestions([{ ...asked, messages: [...asked.messages, { at: at(3000), from: "lead", to: "a", text: "답: 탭" }] }]).length, 0);
  assert.equal(cardQuestions([{ ...asked, status: "done" }]).length, 0);
  assert.equal(cardQuestions([{ ...asked, runId: `session-${asked.runId}` }]).length, 0);
});

test("an approval waits for its own answer; a --question is left to the lead that dispatched the run", () => {
  const asked: RunView = { ...live, messages: [
    { at: at(1000), id: "q1", kind: "approval", from: "a", to: "lead", text: "어떤 방식? [보기: A | B]" },
    { at: at(2000), id: "q2", kind: "question", from: "a", to: "lead", text: "설정은 어디에 있나요?" },
    { at: at(3000), from: "lead", to: "a", text: "다른 안내" },
  ] };
  const led = new Set([asked.runId]);
  const pending = cardQuestions([asked], led);
  // The lead answers q2 itself (ADR-0029); the person is not asked it as well.
  assert.equal(pending.length, 1);
  assert.equal(pending[0].source.kind === "card" && pending[0].source.approval, true);
  assert.equal(pending[0].source.kind === "card" && pending[0].source.messageId, "q1");
  assert.equal(pending[0].questions[0].options.length, 2);
  // An answer to another request, or a message without one, does not answer it.
  const answered: RunView = { ...asked, messages: [...asked.messages, { at: at(4000), from: "lead", to: "a", text: "답", replyTo: "q1" }] };
  assert.equal(cardQuestions([answered], led).length, 0);
});

test("a --question from a run no lead thread dispatched waits on the person", () => {
  const asked: RunView = { ...live, messages: [
    { at: at(1000), id: "q2", kind: "question", from: "a", to: "lead", text: "설정은 어디에 있나요?" },
    { at: at(2000), id: "q3", kind: "question", from: "b", to: "lead", text: "취소한 질문인가요?", recipients: { lead: "cancelled" } },
  ] };
  // Started with `o dispatch` from the CLI: the thread neither planned nor continued it.
  const events = [
    { t: "lead.plan", at: at(0), planId: "p1", plan: { title: "다른 작업", cards: [] }, ok: true },
    { t: "lead.dispatch", at: at(0), planId: "p1", runId: "other-run" },
    { t: "lead.followup", at: at(0), sourceRunId: "other-run", cardId: "a", runId: "followup-run" },
  ];
  const thread = leadItems(events);
  assert.deepEqual([...leadRuns(thread)].sort(), ["followup-run", "other-run"]);
  const [only, ...rest] = cardQuestions([asked], leadRuns(thread));
  assert.equal(rest.length, 0, "a cancelled question asks nothing");
  assert.equal(only.source.kind === "card" && only.source.messageId, "q2");
  assert.equal(only.source.kind === "card" && only.source.approval, false);
  // Once a reply is linked to it, nothing waits.
  assert.equal(cardQuestions([{ ...asked, messages: [...asked.messages, { at: at(3000), from: "lead", to: "a", text: "답: 여기", replyTo: "q2" }] }], leadRuns(thread)).length, 0);
  // The same run dispatched by the thread is the lead's to answer.
  assert.equal(cardQuestions([asked], new Set([asked.runId])).length, 0);
});

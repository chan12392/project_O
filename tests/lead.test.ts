import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { ATTACH, LEAD_EFFORTS, LEAD_VIEW, MODEL_ID, attachProblem, effortFor, fileSize, leadItems, leadView, modelLabel, type LeadEvent, type LeadShow } from "../src/core/lead.ts";
import { createPoll } from "../src/core/poll.ts";

const rust = (path: string) => readFileSync(new URL(`../crates/o-core/src/${path}`, import.meta.url), "utf8");
/** The string list a Rust fn or const returns, e.g. `fn lead_efforts ... &["low", "medium"]`. */
const list = (text: string, after: string) => {
  const body = text.slice(text.indexOf(after));
  return [...(/\[("[^\]]*)\]/.exec(body)?.[1].matchAll(/"([^"]+)"/g) ?? [])].map((m) => m[1]);
};

test("the composer offers the efforts each lead CLI takes, as the O CLI checks them", () => {
  assert.deepEqual(LEAD_EFFORTS.claude, list(rust("providers/claude.rs"), "fn lead_efforts"));
  assert.deepEqual(LEAD_EFFORTS.codex, list(rust("providers/codex.rs"), "fn lead_efforts"));
  assert.deepEqual(LEAD_EFFORTS.agy, list(rust("providers/agy.rs"), "fn lead_efforts"));
});

test("a typed-in model id is checked by the same rule as a card's model", () => {
  const source = /static MODEL: [^\n]*Regex::new\(r"([^"]+)"\)/.exec(rust("policy.rs"))?.[1];
  // The same pattern; a JS character class needs no escape before its literal "[".
  assert.equal(MODEL_ID.source, source?.replace("\\[\\]", "[\\]"));
  for (const ok of ["opus", "gpt-6.1-sol", "claude-opus-5-5[1m]", "models/gemini:x"]) assert.ok(MODEL_ID.test(ok), ok);
  for (const bad of ["", "-opus", ".x", "opus 5", "a;b", "x".repeat(81)]) assert.ok(!MODEL_ID.test(bad), bad);
  assert.equal(modelLabel([{ id: "opus", label: "Opus" }], "opus"), "Opus");
  assert.equal(modelLabel([], "my-model"), "my-model");
  assert.equal(modelLabel([], undefined), undefined);
});

test("attachment limits match the O CLI's", () => {
  const files = rust("lead/files.rs");
  assert.deepEqual(ATTACH.images, list(files, "IMAGE_TYPES"));
  assert.deepEqual(ATTACH.documents, list(files, "DOCUMENT_TYPES"));
  assert.match(files, /MAX_BYTES: u64 = 10 \* 1024 \* 1024/);
  assert.match(files, /MAX_PER_MESSAGE: usize = 5;/);
});

test("a thread folds into messages, replies, plan cards with their run, and errors", () => {
  const events: LeadEvent[] = [
    { t: "lead.start", at: "1", threadId: "th", project: "C:/p" },
    { t: "lead.user", at: "2", text: "해 줘", provider: "claude", effort: "low", attachments: [{ name: "01-a.png", path: "C:/f/01-a.png", size: 3, image: true }] },
    { t: "lead.session", at: "3", provider: "claude", sessionId: "s" },
    { t: "lead.usage", at: "4", provider: "claude", input: 1, cached: 2, output: 3 },
    { t: "lead.reply", at: "5", provider: "claude", text: "카드 한 장" },
    { t: "lead.plan", at: "6", planId: "th-p1", plan: { title: "README", cards: [{ id: "docs", provider: "claude", task: "한 줄" }] }, ok: true, violations: [] },
    { t: "lead.dispatch", at: "7", planId: "th-p1", runId: "r1", clean: true },
    { t: "lead.error", at: "8", text: "총괄을 중지했습니다" },
  ];
  const items = leadItems([...events, { t: "lead.future" }]);
  assert.deepEqual(
    items.map((i) => i.kind),
    ["user", "reply", "plan", "error"],
  );
  const plan = items[2];
  assert.ok(plan.kind === "plan");
  assert.equal(plan.runId, "r1");
  assert.equal(plan.cards[0].id, "docs");
  const user = items[0];
  assert.ok(user.kind === "user" && user.attachments.length === 1);
  // A plan without a title or cards still draws.
  const bare = leadItems([{ t: "lead.plan", at: "1", planId: "p", plan: {}, ok: false, violations: [{ rule: "plan-shape", message: "x" }] } as LeadEvent]);
  assert.ok(bare[0].kind === "plan" && bare[0].title === "제목 없는 계획" && bare[0].cards.length === 0);
});

test("switching the lead keeps the effort, or the nearest one the new CLI takes", () => {
  assert.equal(effortFor("codex", "high"), "high");
  assert.equal(effortFor("codex", "max"), "xhigh");
  assert.equal(effortFor("claude", "minimal"), "low");
  assert.equal(effortFor("claude", undefined), "medium");
});

test("attachments are checked before they are sent", () => {
  assert.equal(attachProblem("a.png", 10, 0), undefined);
  assert.equal(attachProblem("Notes.MD", 10, 4), undefined);
  assert.match(attachProblem("a.exe", 10, 0) ?? "", /이미지/);
  assert.match(attachProblem("noext", 10, 0) ?? "", /이미지/);
  assert.match(attachProblem("big.pdf", ATTACH.maxBytes + 1, 0) ?? "", /10MB/);
  assert.match(attachProblem("a.png", 10, 5) ?? "", /5개/);
  assert.equal(fileSize(900), "900 B");
  assert.equal(fileSize(34 * 1024), "34 KB");
  assert.equal(fileSize(1.25 * 1024 * 1024), "1.3 MB");
});

test("a stored plan of the wrong shape draws as a safe plan with a notice, never throws", () => {
  // As `o lead show` may print a refused plan: the policy stored it with its violations as it came.
  const raw = [
    { t: "lead.plan", at: "1", planId: "th-p1", plan: { title: 42, cards: [null, { id: "ui", provider: "codex", task: "버튼", model: 5 }, 7, "x"] }, ok: false, violations: [{ rule: "plan-shape", message: "제목" }, null, { rule: 3 }] },
    { t: "lead.plan", at: "2", planId: "th-p2", plan: "not an object", ok: true, violations: "none" },
    { t: "lead.plan", at: "3", planId: "th-p3", plan: { title: "  ", cards: { id: "x" } }, ok: true, violations: [] },
    { t: "lead.plan", at: "4", planId: "th-p4", plan: { title: "카드 모양만 이상", cards: [{ id: 1, provider: null, task: ["a"] }] }, ok: true, violations: [] },
  ];
  const items = leadItems(raw);
  assert.equal(items.length, 4);
  const [first, second, third, fourth] = items;
  assert.ok(first.kind === "plan" && second.kind === "plan" && third.kind === "plan" && fourth.kind === "plan");
  assert.equal(first.title, "제목 없는 계획");
  assert.deepEqual(first.cards, [{ id: "ui", provider: "codex", task: "버튼" }]);
  assert.deepEqual(first.violations, [{ rule: "plan-shape", message: "제목" }]);
  assert.ok(first.problems.length > 0);
  // A plan whose shape is wrong is never offered to run, whatever `ok` says.
  for (const plan of [second, third, fourth]) {
    assert.equal(plan.ok, false, plan.planId);
    assert.ok(plan.problems.length > 0, plan.planId);
  }
  assert.deepEqual(second.cards, []);
  assert.deepEqual(second.violations, []);
  assert.equal(third.title, "제목 없는 계획");
  assert.deepEqual(fourth.cards, []);
  // A well-formed plan has no problems.
  const stored: LeadEvent = { t: "lead.plan", at: "5", planId: "th-p5", plan: { title: "README", cards: [{ id: "docs", provider: "claude", task: "한 줄" }] }, ok: true, violations: [] };
  const good = leadItems([stored]);
  assert.ok(good[0].kind === "plan" && good[0].ok && good[0].problems.length === 0);
});

const shown = (project: string, projects: string[]): LeadShow => ({
  thread: { threadId: `t-${project}`, project, busy: null, events: [] },
  projects: projects.map((p) => ({ project: p, threadId: `t-${p}` })),
});

test("the discovered lead projects stay while the chat switches project, so discovery cannot loop", () => {
  // No runs, no chosen project: `o lead show` (no project) finds one, the app switches to it,
  // and the switch must not empty the list it was found in (that sent the app back to "no project").
  let view = leadView(LEAD_VIEW, { type: "project", project: undefined });
  view = leadView(view, { type: "answer", project: undefined, seq: 1, show: shown("C:/a", ["C:/a"]) });
  assert.deepEqual(view.projects.map((p) => p.project), ["C:/a"]);
  view = leadView(view, { type: "project", project: "C:/a" });
  assert.equal(view.show, undefined, "the thread of the folder looked at before is not shown");
  assert.deepEqual(view.projects.map((p) => p.project), ["C:/a"], "switching emptied the discovered list");
  view = leadView(view, { type: "answer", project: "C:/a", seq: 2, show: shown("C:/a", ["C:/a"]) });
  assert.equal(view.show?.thread?.project, "C:/a");
});

test("only the newest lead answer lands; one for another project or an older request is dropped", () => {
  let view = leadView(LEAD_VIEW, { type: "project", project: "C:/a" });
  view = leadView(view, { type: "answer", project: "C:/a", seq: 3, show: shown("C:/a", ["C:/a", "C:/new"]) });
  // The 0.4 s check after sending, answered late, must not undo the newer answer.
  const stale = leadView(view, { type: "answer", project: "C:/a", seq: 2, show: shown("C:/a", ["C:/a"]) });
  assert.equal(stale, view);
  assert.equal(leadView(view, { type: "failed", project: "C:/a", seq: 1, error: "늦은 실패" }), view);
  // An answer for the project looked at before the switch.
  const other = leadView(leadView(view, { type: "project", project: "C:/b" }), { type: "answer", project: "C:/a", seq: 4, show: shown("C:/a", []) });
  assert.equal(other.show, undefined);
  // A newer failure after a good read keeps what was read and says it failed.
  const failed = leadView(view, { type: "failed", project: "C:/a", seq: 5, error: "o lead show 실패" });
  assert.equal(failed.show, view.show);
  assert.equal(failed.failed, "o lead show 실패");
  assert.equal(leadView(failed, { type: "answer", project: "C:/a", seq: 6, show: shown("C:/a", ["C:/a"]) }).failed, undefined);
});

test("the lead poll never runs two loops: a wake while a check is in flight starts nothing new", async () => {
  const timers = new Map<number, () => void>();
  let next = 0;
  let hidden = false;
  let calls = 0;
  let release: () => void = () => {};
  const poll = createPoll({
    run: () => {
      calls += 1;
      return new Promise<void>((resolve) => (release = resolve));
    },
    delay: 2000,
    hidden: () => hidden,
    setTimer: (fn) => (timers.set(++next, fn), next),
    clearTimer: (id) => void timers.delete(id),
  });
  poll.wake();
  // Hidden and shown again, several times, while the first check is still out.
  for (let i = 0; i < 3; i++) {
    hidden = true;
    poll.wake();
    hidden = false;
    poll.wake();
  }
  assert.equal(calls, 1);
  release();
  await new Promise((r) => setTimeout(r, 0));
  assert.equal(timers.size, 1, "one loop, one timer");
  [...timers.values()][0]();
  assert.equal(calls, 2);
  poll.stop();
  release();
  await new Promise((r) => setTimeout(r, 0));
  assert.equal(timers.size, 0, "stopped: nothing is scheduled");
});

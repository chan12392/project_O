import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import {
  ATTACH,
  EMPTY_DRAFT,
  LEAD_EFFORTS,
  LEAD_VIEW,
  MODEL_ID,
  attachProblem,
  canLead,
  draftKey,
  earlierPlanOutcome,
  draftOf,
  editDraft,
  effortFor,
  fileSize,
  leadItems,
  leadPlans,
  leadProgressSummary,
  leadReadProblem,
  leadToolLabel,
  leadView,
  modelLabel,
  planIsStale,
  restoreDraft,
  takeDraft,
  taskIsLong,
  uploadDone,
  type Drafts,
  type LeadEvent,
  type LeadShow,
} from "../src/core/lead.ts";
import { createPoll } from "../src/core/poll.ts";

test("tool activity shows concise real file names and does not pretend glob patterns are files", () => {
  assert.equal(leadToolLabel("Read", "docs/워크트리.md"), "Read 워크트리.md");
  assert.equal(leadToolLabel("Write", "C:\\project\\game.py"), "Write game.py");
  assert.equal(leadToolLabel("edit", '[{"path":"src/app.ts","kind":"update"}]'), "Edit app.ts");
  assert.equal(leadToolLabel("Glob", "**/*"), "Glob 파일 찾기");
  assert.equal(leadToolLabel("Read", ""), "Read");
  assert.equal(leadProgressSummary({ commands: 7, edits: 3, reads: 0, searches: 1 }), "명령 7회 · 파일 변경 3회 · 검색 1회");
  assert.equal(leadProgressSummary(undefined), "");
});

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

test("a card message stays distinct from a user instruction", () => {
  const items = leadItems([{ t: "lead.worker", at: "1", runId: "r1", cardId: "a", messageId: "m1", text: "확인이 필요합니다" } satisfies LeadEvent]);
  assert.deepEqual(items, [{ kind: "worker", at: "1", runId: "r1", cardId: "a", text: "확인이 필요합니다" }]);
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

test("plan resources remain visible and malformed declarations prevent dispatch", () => {
  const events = [
    { t: "lead.plan", at: "1", planId: "good", plan: { title: "good", cards: [{ id: "a", provider: "claude", task: "x", resources: [{ kind: "project-path", key: "src", mode: "write" }] }] }, ok: true, violations: [] },
    { t: "lead.plan", at: "2", planId: "bad", plan: { title: "bad", cards: [{ id: "a", provider: "claude", task: "x", resources: [{ kind: "external", key: "db", mode: "maybe" }] }] }, ok: true, violations: [] },
  ] as unknown as LeadEvent[];
  const [good, bad] = leadItems(events);
  assert.ok(good.kind === "plan" && bad.kind === "plan");
  assert.deepEqual(good.cards[0].resources, [{ kind: "project-path", key: "src", mode: "write" }]);
  assert.equal(good.ok, true);
  assert.equal(bad.ok, false);
  assert.match(bad.problems.join(" "), /자원 선언/);
});

test("an earlier plan stays readable after a newer one, and only the latest can run (#40)", () => {
  const items = leadItems([
    { t: "lead.user", at: "1", text: "로그인 고쳐 줘", provider: "claude", attachments: [] },
    {
      t: "lead.plan",
      at: "2",
      planId: "th-a",
      plan: { title: "계획 A", cards: [{ id: "api", provider: "codex", task: "토큰 검사" }, { id: "ui", provider: "openclaw", task: "로그인 화면", depends_on: ["api"] }] },
      ok: false,
      violations: [{ rule: "provider", cardId: "ui", message: "허용하지 않는 프로바이더" }],
    },
    { t: "lead.user", at: "3", text: "Claude로 바꿔 줘", provider: "claude", attachments: [] },
    {
      t: "lead.plan",
      at: "4",
      planId: "th-b",
      plan: { title: "계획 B", cards: [{ id: "fix", provider: "claude", task: "토큰과 화면" }, { id: "test", provider: "codex", task: "테스트", depends_on: ["fix"] }] },
      ok: false,
      violations: [{ rule: "daily-budget", cardId: "test", message: "오늘 예산 소진" }],
    },
  ] as LeadEvent[]);
  const { latest, earlier } = leadPlans(items);
  assert.equal(latest?.planId, "th-b");
  assert.deepEqual(earlier.map((plan) => plan.planId), ["th-a"]);
  // A's own cards, dependencies and violation, not B's.
  const [a] = earlier;
  assert.deepEqual(a.cards.map((card) => [card.id, card.task, card.depends_on]), [["api", "토큰 검사", undefined], ["ui", "로그인 화면", ["api"]]]);
  assert.deepEqual(a.violations, [{ rule: "provider", cardId: "ui", message: "허용하지 않는 프로바이더" }]);
  assert.equal(earlierPlanOutcome(a), "정책 차단");
  // A is out of date, so it is not dispatched; B still is the one to run.
  assert.equal(planIsStale(items, "th-a"), true);
  assert.equal(planIsStale(items, "th-b"), false);
  assert.equal(earlierPlanOutcome({ ...a, ok: true, violations: [] }), "실행하지 않음");
  assert.equal(earlierPlanOutcome({ ...a, runId: "r1" }), "실행 r1");
  // The earlier-designs list draws the read-only details; only the latest gets the PlanCard with its run button.
  const source = (path: string) => readFileSync(new URL(`../app/src/features/${path}`, import.meta.url), "utf8");
  const work = source("work/WorkTab.tsx");
  assert.match(work, /<PlanCard plan=\{latest\}/);
  assert.equal(work.match(/<PlanCard\b/g)?.length, 1);
  assert.match(work, /<PlanDetails plan=\{plan\} \/>/);
  const thread = source("chat/LeadThread.tsx");
  const details = thread.slice(thread.indexOf("export function PlanDetails"), thread.indexOf("function CardTask"));
  assert.ok(details.length > 0);
  assert.doesNotMatch(details, /dispatch|BUTTON_PRIMARY/);
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

test("each project keeps its own composer draft; a late upload lands only where and when it started", () => {
  const a = draftKey("C:\\work\\A");
  const b = draftKey("C:/work/b/");
  assert.equal(draftKey("c:/work/a/"), a, "the same folder written another way is the same draft");
  let drafts: Drafts = {};
  drafts = editDraft(drafts, a, (d) => ({ text: "A에게", files: [...d.files, { key: "f1", name: "a.png", size: 3, image: true }] }));
  const started = draftOf(drafts, a).gen;
  // Switched to B: nothing of A shows there.
  assert.deepEqual(draftOf(drafts, b), EMPTY_DRAFT);
  drafts = editDraft(drafts, b, () => ({ text: "B에게" }));
  // A's upload finishes while B is shown: it lands in A's draft, not B's.
  const saved = { name: "01-a.png", path: "C:/lead/tA/01-a.png", size: 3, image: true };
  drafts = uploadDone(drafts, a, started, "f1", { saved });
  assert.deepEqual(draftOf(drafts, a).files[0].saved, saved);
  assert.deepEqual(draftOf(drafts, b).files, []);
  // What B sends carries only B's draft.
  const taken = takeDraft(drafts, b);
  assert.deepEqual(taken.draft, { ...draftOf(drafts, b) });
  assert.equal(taken.draft.files.length, 0);
  drafts = taken.drafts;
  assert.equal(draftOf(drafts, b).text, "");
  // An upload started before a send finishes after it: the sent draft does not get it back.
  drafts = editDraft(drafts, b, (d) => ({ files: [{ key: "f2", name: "b.md", size: 1, image: false }] }));
  const before = draftOf(drafts, b).gen;
  drafts = takeDraft(drafts, b).drafts;
  drafts = uploadDone(drafts, b, before, "f2", { saved: { name: "01-b.md", path: "C:/lead/tB/01-b.md", size: 1, image: false } });
  assert.deepEqual(draftOf(drafts, b).files, []);
});

test("a send that fails gives back the message and its attachments, not over a new draft", () => {
  const key = draftKey("C:/work/a");
  const files = [{ key: "f1", name: "a.png", size: 3, image: true, saved: { name: "01-a.png", path: "C:/l/01-a.png", size: 3, image: true } }];
  let drafts = editDraft({}, key, () => ({ text: "보낼 글", files }));
  const { drafts: sent, draft } = takeDraft(drafts, key);
  drafts = restoreDraft(sent, key, draft);
  assert.equal(draftOf(drafts, key).text, "보낼 글");
  assert.deepEqual(draftOf(drafts, key).files, files);
  // Something typed while it was sending stays.
  drafts = restoreDraft(editDraft(sent, key, () => ({ text: "새 글" })), key, draft);
  assert.equal(draftOf(drafts, key).text, "새 글");
  assert.deepEqual(draftOf(drafts, key).files, files);
});

test("onboarding and the composer agree on which agents can lead", () => {
  const agent = (id: string, installed: boolean, loggedIn: boolean | null) => ({ id, installed, loggedIn });
  assert.ok(canLead(agent("claude", true, true)));
  // agy reports no sign-in state when it cannot read its quota: unknown is not "signed out".
  assert.ok(canLead(agent("agy", true, null)));
  assert.ok(!canLead(agent("codex", true, false)));
  assert.ok(!canLead(agent("claude", false, null)));
  assert.ok(!canLead(agent("openclaw", true, true)));
  // Both screens decide with this one rule (they used two, so onboarding said "ready" while the composer could not send).
  const app = (path: string) => readFileSync(new URL(`../app/src/features/${path}`, import.meta.url), "utf8");
  for (const screen of ["chat/Composer.tsx", "settings/AgentSetup.tsx"]) assert.match(app(screen), /\bcanLead\b/, screen);
});

test("a thread that cannot be read says what to check; a long card task can be read in full", () => {
  const old = leadReadProblem("Command failed: o lead show --json\nusage: o <command>\n  runs …");
  assert.match(old, /O CLI/);
  assert.match(old, /Command failed/);
  assert.doesNotMatch(leadReadProblem("총괄 대화 목록을 읽지 못했습니다"), /O CLI/);
  assert.equal(taskIsLong("README 한 줄"), false);
  assert.equal(taskIsLong("가".repeat(121)), true);
  assert.equal(taskIsLong("한 줄\n두 줄\n세 줄\n네 줄"), true);
});

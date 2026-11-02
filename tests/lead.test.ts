import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { ATTACH, LEAD_EFFORTS, attachProblem, effortFor, fileSize, leadItems, type LeadEvent } from "../src/core/lead.ts";

const rust = (path: string) => readFileSync(new URL(`../crates/o-core/src/${path}`, import.meta.url), "utf8");
/** The string list a Rust fn or const returns, e.g. `fn lead_efforts ... &["low", "medium"]`. */
const list = (text: string, after: string) => {
  const body = text.slice(text.indexOf(after));
  return [...(/\[("[^\]]*)\]/.exec(body)?.[1].matchAll(/"([^"]+)"/g) ?? [])].map((m) => m[1]);
};

test("the composer offers the efforts each lead CLI takes, as the O CLI checks them", () => {
  assert.deepEqual(LEAD_EFFORTS.claude, list(rust("providers/claude.rs"), "fn lead_efforts"));
  assert.deepEqual(LEAD_EFFORTS.codex, list(rust("providers/codex.rs"), "fn lead_efforts"));
  // Antigravity cannot lead (no read-only mode O can rely on).
  assert.ok(!rust("providers/agy.rs").includes("fn lead_efforts"));
  assert.ok(!("agy" in LEAD_EFFORTS));
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

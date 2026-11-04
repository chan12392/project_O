import assert from "node:assert/strict";
import { test } from "node:test";

import { leadItems, type LeadApproval } from "../src/core/lead.ts";
import { approvalView, decisionText, detailIsLong, fileName, lineCounts, remainingText, secondsLeft, stepLine, workSummary } from "../src/core/leadWork.ts";

const step = (over: Record<string, unknown>) => ({ t: "lead.step", at: "2026-11-04T00:00:01.000Z", id: "s", kind: "read", label: "C:/p/src/a.ts", status: "done", ...over });

test("steps read as short lines: what it does now, then what it did", () => {
  assert.deepEqual(stepLine({ id: "1", kind: "read", label: "C:\\p\\src\\a.ts", file: "C:\\p\\src\\a.ts", status: "running" }), { verb: "읽는 중", target: "a.ts", running: true });
  assert.equal(stepLine({ id: "2", kind: "run", label: "npm test\nmore", status: "running" }).verb, "실행 중");
  assert.equal(stepLine({ id: "2", kind: "run", label: "npm test\nmore", status: "done" }).target, "npm test");
  assert.equal(stepLine({ id: "3", kind: "web", label: "tauri 2 capabilities", status: "running" }).verb, "웹 검색 중");
  assert.equal(stepLine({ id: "4", kind: "web", label: "https://example.com/a", status: "done" }).verb, "열어 봄");
  const edit = stepLine({ id: "5", kind: "edit", label: "C:/p/README.md", file: "C:/p/README.md", added: 200, removed: 199, status: "done" });
  assert.deepEqual([edit.target, lineCounts(edit.added, edit.removed)], ["README.md", "+200 −199"]);
  assert.equal(stepLine({ id: "6", kind: "run", label: "rm x", status: "denied" }).note, "거부됨");
  assert.equal(stepLine({ id: "7", kind: "mystery", label: "Task", status: "incomplete" }).verb, "사용함");
  assert.equal(stepLine({ id: "7", kind: "mystery", label: "Task", status: "incomplete" }).note, "중단됨");
  assert.equal(lineCounts(undefined, 3), "−3");
  assert.equal(lineCounts(2, undefined), "+2");
  assert.equal(lineCounts(undefined, undefined), "");
  assert.equal(fileName("C:/a/b/"), "b");
});

test("a turn's steps and approvals fold into one work item above its answer", () => {
  const items = leadItems([
    { t: "lead.user", at: "2026-11-04T00:00:00.000Z", text: "고쳐 줘", provider: "claude" },
    step({ id: "r1" }),
    { t: "lead.approval", at: "2026-11-04T00:00:02.000Z", id: "t-a1", provider: "claude", kind: "file", tool: "Write", summary: "C:/p/a.ts", files: [{ path: "C:/p/a.ts", added: 2, removed: 0 }], expires: "x" },
    { t: "lead.decision", at: "2026-11-04T00:00:03.000Z", id: "t-a1", decision: "allow", by: "user" },
    step({ id: "e1", kind: "edit", file: "C:/p/a.ts", added: 2, removed: 0 }),
    { t: "lead.approval", at: "2026-11-04T00:00:04.000Z", id: "t-a2", provider: "claude", kind: "command", tool: "Bash", summary: "git push origin main", expires: "x" },
    { t: "lead.decision", at: "2026-11-04T00:00:05.000Z", id: "t-a2", decision: "deny", by: "guard", reason: "보호 브랜치" },
    { t: "lead.reply", at: "2026-11-04T00:00:06.000Z", provider: "claude", text: "고쳤습니다" },
    { t: "lead.user", at: "2026-11-04T00:01:00.000Z", text: "또", provider: "claude" },
    step({ id: "r2", at: "2026-11-04T00:01:01.000Z" }),
  ] as unknown as { t: string }[]);
  assert.deepEqual(items.map((i) => i.kind), ["user", "work", "reply", "user", "work"]);
  const work = items[1];
  assert.ok(work.kind === "work");
  assert.equal(work.entries.length, 4);
  assert.deepEqual(work.entries.filter((e) => e.type === "approval").map((e) => e.type === "approval" && decisionText(e.decision, e.by)), ["허용함", "O 정책이 거부"]);
  assert.equal(workSummary(work.entries), "작업 2단계 · 파일 1개 +2 −0 · 거부 1건");
  const second = items[4];
  assert.ok(second.kind === "work");
  assert.equal(workSummary(second.entries), "작업 1단계");
  assert.equal(decisionText(undefined, undefined), "승인 기다리는 중");
  assert.equal(decisionText("timeout", "timeout"), "시간 초과로 거부");
});

test("malformed step events are left out", () => {
  const items = leadItems([{ t: "lead.step", at: "x", id: 3, label: null }, { t: "lead.decision", at: "x", id: "none", decision: "allow", by: "user" }] as unknown as { t: string }[]);
  assert.deepEqual(items, []);
});

const approval = (over: Partial<LeadApproval> = {}): LeadApproval => ({
  id: "t-a1", at: "2026-11-04T00:00:00.000Z", expires: "2026-11-04T00:10:00.000Z", provider: "claude", kind: "command", tool: "Bash",
  summary: "npm test", detail: "npm test -- --watch=false", cwd: "C:/p", reason: "테스트 확인", ...over,
});

test("the approval card shows the oldest request in full and counts down to its refusal", () => {
  const start = Date.parse("2026-11-04T00:00:00.000Z");
  const view = approvalView([approval(), approval({ id: "t-a2" })], start + 48_000);
  assert.ok(view);
  assert.deepEqual([view.id, view.title, view.detail, view.cwd, view.more], ["t-a1", "명령 실행", "npm test -- --watch=false", "C:/p", 1]);
  assert.equal(remainingText(view.secondsLeft), "9분 12초 남음");
  assert.equal(remainingText(secondsLeft("2026-11-04T00:10:00.000Z", start + 560_000)), "40초 남음");
  assert.equal(remainingText(secondsLeft("2026-11-04T00:10:00.000Z", start + 700_000)), "곧 거부됩니다");
  assert.equal(secondsLeft("not a time", start), 0);
  assert.equal(approvalView([], start), undefined);
  assert.equal(approvalView(undefined, start), undefined);
});

test("a file request lists each file with its counts instead of the raw request", () => {
  const view = approvalView([approval({ kind: "file", tool: "Write", summary: "C:/p/a.ts", detail: "+a\n+b", files: [{ path: "C:\\p\\a.ts", added: 2, removed: 1, diff: "+a\n+b" }] })], 0);
  assert.ok(view);
  assert.deepEqual(view.files.map((f) => [f.name, f.counts]), [["a.ts", "+2 −1"]]);
  assert.equal(approvalView([approval({ kind: "file", files: [{ path: "new.ts", added: 2 }] })], 0)?.files[0].counts, "+2");
  assert.equal(view.detail, "");
  assert.equal(approvalView([approval({ kind: "web", tool: "WebFetch", summary: "https://x.dev", detail: "요약", cwd: null, reason: null })], 0)?.title, "웹 요청");
  assert.equal(approvalView([approval({ kind: "other", files: undefined })], 0)?.title, "도구 사용");
  assert.ok(detailIsLong("x\n".repeat(9)));
  assert.ok(!detailIsLong("npm test"));
});

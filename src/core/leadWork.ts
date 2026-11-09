/**
 * A lead turn's work as the chat draws it (ADR-0008 개정 3): its steps while it runs ("읽는 중
 * a.ts", "'a.ts' +12 −3"), the same list folded above the answer afterwards ("작업 n단계"), and
 * the action it waits on above the composer. Screens draw what these return; nothing here decides.
 */
import type { LeadApproval, LeadApprovalFile, LeadStep, LeadWorkEntry } from "./lead.ts";

/** The last part of a path, either slash. */
export function fileName(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts.at(-1) ?? path;
}

/** A step as one line: what it does or did, what it worked on, and an edit's line counts. */
export interface StepLine {
  verb: string;
  target: string;
  added?: number;
  removed?: number;
  /** How it ended when not as asked: 실패 · 거부됨 · 중단됨. */
  note?: string;
  running: boolean;
}

const VERBS: Record<string, [running: string, done: string]> = {
  read: ["읽는 중", "읽음"],
  search: ["검색 중", "검색함"],
  run: ["실행 중", "실행함"],
  edit: ["수정 중", "수정함"],
  web: ["웹 검색 중", "웹 검색함"],
  other: ["사용 중", "사용함"],
};

const NOTES: Record<string, string> = { failed: "실패", denied: "거부됨", incomplete: "중단됨" };

export function stepLine(step: LeadStep): StepLine {
  const running = step.status === "running";
  const kind = step.kind in VERBS ? step.kind : "other";
  const url = kind === "web" && /^https?:\/\//i.test(step.label);
  const [doing, did] = url ? ["여는 중", "열어 봄"] : VERBS[kind];
  const path = kind === "read" || kind === "edit";
  const target = path ? fileName(step.file || step.label) : step.label.split("\n")[0];
  const line: StepLine = { verb: running ? doing : did, target, running };
  if (kind === "edit" && typeof step.added === "number" && typeof step.removed === "number") Object.assign(line, { added: step.added, removed: step.removed });
  if (NOTES[step.status]) line.note = NOTES[step.status];
  return line;
}

/** "+12 −3" with a real minus sign; empty without counts. */
export function lineCounts(added?: number, removed?: number): string {
  return [typeof added === "number" ? `+${added}` : "", typeof removed === "number" ? `−${removed}` : ""].filter(Boolean).join(" ");
}

const APPROVAL_KIND: Record<LeadApproval["kind"], string> = { command: "명령 실행", file: "파일 변경", web: "웹 요청", other: "도구 사용" };

export function approvalKind(kind: string): string {
  return APPROVAL_KIND[kind as LeadApproval["kind"]] ?? "도구 사용";
}

const DECISION_WORD: Record<string, string> = { allow: "허용함", deny: "거부함", timeout: "시간 초과로 거부", stopped: "중지됨" };

/** What became of an approval, for the work list: who decided and how. */
export function decisionText(decision: string | undefined, by: string | undefined): string {
  if (!decision) return "승인 기다리는 중";
  if (decision === "deny" && by === "guard") return "O 정책이 거부";
  return DECISION_WORD[decision] ?? decision;
}

/** The folded list's title: "작업 7단계 · 파일 2개 +12 −3". Steps count; approvals do not. */
export function workSummary(entries: readonly LeadWorkEntry[]): string {
  const steps = entries.flatMap((e) => (e.type === "step" ? [e.step] : []));
  const edits = steps.filter((s) => s.kind === "edit" && s.status === "done");
  const files = new Set(edits.map((s) => s.file || s.label));
  const added = edits.reduce((n, s) => n + (s.added ?? 0), 0);
  const removed = edits.reduce((n, s) => n + (s.removed ?? 0), 0);
  const parts = [`작업 ${steps.length}단계`];
  if (files.size > 0) parts.push(`파일 ${files.size}개 ${lineCounts(added, removed)}`);
  const denied = entries.filter((e) => e.type === "approval" && e.decision && e.decision !== "allow").length;
  if (denied > 0) parts.push(`거부 ${denied}건`);
  return parts.join(" · ");
}

/** Seconds left before a request is refused for waiting too long (never below zero). */
export function secondsLeft(expires: string, now: number): number {
  const end = Date.parse(expires);
  return Number.isFinite(end) ? Math.max(0, Math.ceil((end - now) / 1000)) : 0;
}

/** "9분 12초 남음", "40초 남음", or "곧 거부됩니다" at the end. */
export function remainingText(seconds: number): string {
  if (seconds <= 0) return "곧 거부됩니다";
  const minutes = Math.floor(seconds / 60);
  const rest = seconds % 60;
  return minutes > 0 ? `${minutes}분 ${rest}초 남음` : `${rest}초 남음`;
}

/** What the approval card shows for one request, oldest waiting first. */
export interface ApprovalView {
  id: string;
  title: string;
  summary: string;
  /** The whole command, or the request as sent; shown folded when long. */
  detail: string;
  cwd?: string;
  reason?: string;
  files: (LeadApprovalFile & { name: string; counts: string })[];
  secondsLeft: number;
  /** How many more wait after this one. */
  more: number;
}

export function approvalView(approvals: readonly LeadApproval[] | undefined, now: number): ApprovalView | undefined {
  const first = approvals?.[0];
  if (!first) return undefined;
  const files = (first.files ?? []).map((f) => ({ ...f, name: fileName(f.path), counts: lineCounts(f.added, f.removed) }));
  return {
    id: first.id,
    title: approvalKind(first.kind),
    summary: first.summary,
    detail: first.kind === "file" && files.length > 0 ? "" : (first.detail ?? ""),
    cwd: first.cwd ?? undefined,
    reason: first.reason ?? undefined,
    files,
    secondsLeft: secondsLeft(first.expires, now),
    more: approvals.length - 1,
  };
}

/** The notification for an approval request that just came in. */
export function approvalNotice(view: Pick<ApprovalView, "title" | "summary" | "more">): { title: string; body: string } {
  return { title: `총괄 ${view.title} 승인 요청`, body: `${view.summary}${view.more > 0 ? ` 외 ${view.more}개 대기` : ""}` };
}

/** A long detail starts folded: more than this many lines or characters. */
export function detailIsLong(detail: string): boolean {
  return detail.length > 400 || detail.split("\n").length > 8;
}

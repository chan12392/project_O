/**
 * The lead chat (ADR-0008) as screens see it: the thread events `o lead show --json`
 * prints (crates/o-core/src/lead), folded into what the chat draws, and the choices
 * the composer offers. Lead events are their own small type, not run events.
 */
import type { Violation } from "./events.ts";
import type { ProviderId } from "./plan.ts";

export interface LeadAttachment {
  name: string;
  path: string;
  size: number;
  image: boolean;
}

export type LeadViolation = Violation;

export interface LeadProgressCounts { reads: number; searches: number; edits: number; commands: number }

/** One step of a turn (crates/o-core/src/lead/steps.rs `Step`): live in `thread.steps`, finished ones as `lead.step`. */
export interface LeadStep {
  id: string;
  /** read · search · run · edit · web · other (an unknown kind shows as other). */
  kind: string;
  label: string;
  file?: string;
  added?: number;
  removed?: number;
  /** running · done · failed · denied · incomplete. */
  status: string;
}

export interface LeadApprovalFile { path: string; added?: number; removed?: number; diff?: string }

/**
 * An action a normal-mode lead waits on (crates/o-core/src/lead/approvals.rs): what it asks,
 * as the CLI sent it (`detail`, data), until `expires`. Only the person's 허용/거부 decides it.
 */
export interface LeadApproval {
  id: string;
  at: string;
  expires: string;
  provider: ProviderId;
  kind: "command" | "file" | "web" | "other";
  tool: string;
  summary: string;
  detail?: string;
  cwd?: string | null;
  reason?: string | null;
  files?: LeadApprovalFile[];
}

/** One line of a turn's work list: a step, or an approval and what became of it. */
export type LeadWorkEntry =
  | { type: "step"; step: LeadStep }
  | { type: "approval"; id: string; kind: LeadApproval["kind"]; summary: string; files: LeadApprovalFile[]; decision?: string; by?: string; reason?: string };
export interface DesignNote { id: string; quote: string; anchor?: string; occurrence?: number; tag: string; text: string; sent: boolean }
export type DesignNoteChange = { op: "add" | "update"; note: DesignNote } | { op: "delete"; id: string };

/** Counts are tool calls, not a claim that the work is still running or succeeded. */
export function leadProgressSummary(counts: LeadProgressCounts | null | undefined): string {
  if (!counts) return "";
  return [["명령", counts.commands], ["파일 변경", counts.edits], ["읽기", counts.reads], ["검색", counts.searches]]
    .filter(([, count]) => typeof count === "number" && count > 0)
    .map(([label, count]) => `${label} ${count}회`).join(" · ");
}

/** A short, factual tool line; a search pattern is not a file that was read. */
export function leadToolLabel(name: string, summary: string): string {
  const kind = name.toLowerCase();
  if (kind === "glob") return "Glob 파일 찾기";
  const action = ({ read: "Read", read_file: "Read", edit: "Edit", edit_file: "Edit", write: "Write", write_file: "Write" } as Record<string, string>)[kind];
  let detail = summary.trim();
  if (action && detail.startsWith("[")) {
    try {
      const changes: unknown = JSON.parse(detail);
      if (Array.isArray(changes)) detail = changes.flatMap((item: unknown) => typeof item === "object" && item !== null && "path" in item && typeof item.path === "string" ? [item.path.split(/[\\/]/).pop()] : []).filter(Boolean).join(", ");
    } catch { /* A truncated tool summary is still displayable. */ }
  } else if (action) detail = detail.split(/[\\/]/).pop() ?? detail;
  if (detail.length > 56) detail = `${detail.slice(0, 53)}...`;
  return [action ?? name, detail].filter(Boolean).join(" ");
}

export interface PlanCard {
  id: string;
  provider: string;
  role?: string;
  model?: string;
  effort?: string;
  task: string;
  depends_on?: string[];
  resources?: { kind: "project-path" | "external"; key: string; mode: "read" | "write" }[];
}

/** One question the lead asks (crates/o-core/src/lead/prompt.rs `AskQuestion`). */
export interface AskQuestion {
  header?: string;
  question: string;
  options: { label: string; description?: string }[];
  multi?: boolean;
}

export type LeadEvent =
  | { t: "lead.start"; at: string; threadId: string; project: string }
  /** `origin`: absent for the person; "overall" for a request the overall lead passed on, "review" for O's review request (ADR-0032, ADR-0036). */
  | { t: "lead.user"; at: string; text: string; attachments?: LeadAttachment[]; provider: ProviderId; model?: string; effort?: string; origin?: string }
  /** Requests the overall lead proposes; they go on when the person presses 요청 전달 (ADR-0036). */
  | { t: "lead.forward-plan"; at: string; proposalId: string; items: LeadForwardItem[]; provider: ProviderId; model?: string; effort?: string }
  /** A request passed on to a project lead; `revision` names the plan an overall review asked to revise. */
  | { t: "lead.forward"; at: string; project: string; text: string; proposal?: string; revision?: string }
  /** The thread's `index`-th lead.forward did not start (`error`), or was sent again (no `error`); the last one counts (#96). */
  | { t: "lead.forward-start"; at: string; index: number; error?: string }
  /** The overall lead's review of a project lead's plan, in both threads (ADR-0036). */
  | { t: "lead.review"; at: string; planId: string; project: string; verdict: LeadVerdict; notes?: string }
  | { t: "lead.worker"; at: string; runId: string; cardId: string; messageId: string; text: string }
  | { t: "lead.session"; at: string; provider: ProviderId; sessionId: string }
  | { t: "lead.reply"; at: string; provider: ProviderId; text: string; reportedFiles?: string[] }
  | { t: "lead.design"; at: string; design: { goal: string; decisions: string[]; questions: string[] } }
  /** Questions in place of an answer (an `o-ask` block); the next lead.user carries the picks. */
  | { t: "lead.ask"; at: string; askId: string; provider: ProviderId; intro?: string; questions: AskQuestion[] }
  | { t: "lead.plan"; at: string; planId: string; plan: { title?: string; project?: string; cards?: PlanCard[] }; ok: boolean; violations: LeadViolation[]; notices?: string[] }
  | { t: "lead.dispatch"; at: string; planId: string; runId: string; clean: boolean }
  | { t: "lead.followup"; at: string; sourceRunId: string; cardId: string; runId: string }
  | { t: "lead.autorun"; at: string; planId: string; reason?: string | null }
  | { t: "lead.usage"; at: string; provider: ProviderId; input: number; cached: number; output: number }
  | { t: "lead.warning" | "lead.error"; at: string; text: string }
  /** A CLI session got the turn whose lead.user is event `through`; the chat draws nothing for it. */
  | { t: "lead.seen"; at: string; provider: ProviderId; sessionId: string; through: number }
  | ({ t: "lead.approval" } & LeadApproval)
  /** allow · deny · timeout · stopped, by user · guard · timeout · stop · o. */
  | { t: "lead.decision"; at: string; id: string; decision: string; by: string; reason?: string }
  | ({ t: "lead.step"; at: string } & LeadStep);

/** What `o lead show --json` prints. `unknown` means a previous turn's exit is unconfirmed. */
export interface LeadShow {
  thread: { threadId: string; project: string; busy: ProviderId | "starting" | "unknown" | null; progress?: { name: string; summary: string }[]; progressCounts?: LeadProgressCounts | null; partial?: string; steps?: LeadStep[]; approvals?: LeadApproval[]; notes?: DesignNote[]; notesError?: string | null; events: LeadEvent[] } | null;
  /** The project's open conversations, in the order they were opened (ADR-0034); absent in older answers. */
  threads?: LeadTab[];
  projects: { project: string; threadId: string; /** How the project's tabs add up (waiting > working > idle); absent in older answers. */ state?: LeadTab["state"] }[];
}

/** One open lead conversation — a tab on screen (ADR-0034). */
export interface LeadTab {
  threadId: string;
  /** The first request's first line, or "새 총괄" before one. */
  title: string;
  /** working: answering or its dispatched runs go; waiting: it needs the person; idle: otherwise. */
  state: "working" | "waiting" | "idle";
  /** Who answers on it now; "unknown" when a previous turn's exit is unconfirmed. */
  busy: ProviderId | "starting" | "unknown" | null;
  /** The one tab that may edit the project directly; the rest are design-only. */
  writer: boolean;
  startedAt: string;
}

/** A project's open tabs cap (`lead::MAX_OPEN`). */
export const LEAD_TAB_MAX = 4;

/** What the tab's status dot says: waiting means the person's action is needed (approval, question, unconfirmed exit). */
export const LEAD_TAB_WORD: Record<LeadTab["state"], string> = { working: "작업 중", waiting: "확인 필요", idle: "대기" };

/**
 * The tab a `leadShow` answer shows: the picked one while it is still open; the answer's own
 * thread when it is not (closed or never opened — the backend falls back to the project's
 * current conversation). An old answer without `threads` has one tab: its thread (ADR-0034).
 */
export function leadTabOf(show: LeadShow | undefined, picked: string | undefined): string | undefined {
  if (!show || (picked !== undefined && show.threads?.some((tab) => tab.threadId === picked))) return picked;
  return show.thread?.threadId;
}

/** A tab's title when only the thread is known: its first request's first line, or "새 총괄". */
export function leadTabTitle(events: readonly LeadEvent[]): string {
  const first = events.find((e) => e.t === "lead.user" && typeof (e as { text?: unknown }).text === "string") as { text: string } | undefined;
  const line = first?.text.split("\n", 1)[0].trim() ?? "";
  return line ? line.slice(0, 24) : "새 총괄";
}

/** A shown thread's state when the answer names none (`threads` absent): waiting > working > idle. */
export function threadState(thread: NonNullable<LeadShow["thread"]>): LeadTab["state"] {
  if (thread.busy === "unknown" || (thread.approvals?.length ?? 0) > 0) return "waiting";
  return thread.busy ? "working" : "idle";
}

/** The open tabs an answer draws: `threads`, or the one thread of an older answer (its tab is the writer). */
export function leadTabsOf(show: LeadShow | undefined): LeadTab[] {
  if (!show) return [];
  if (show.threads) return show.threads;
  const thread = show.thread;
  return thread ? [{ threadId: thread.threadId, title: leadTabTitle(thread.events), state: threadState(thread), busy: thread.busy, writer: true, startedAt: "" }] : [];
}

/**
 * What the chat knows of the lead (`useLead`): the answer for the project looked at, and,
 * apart from it, every project with a lead thread. Switching project clears the former only:
 * the list is how the app found a project to switch to, and emptying it on the switch made the
 * app forget that project and look again, over and over. Each check carries a sequence number;
 * an answer older than one already applied, or for a project no longer looked at, is dropped.
 */
export interface LeadView {
  project?: string;
  /** The tab the project is asked for; undefined follows the answer's own thread (ADR-0034). */
  thread?: string;
  show?: LeadShow;
  projects: LeadShow["projects"];
  failed?: string;
  seq: number;
}

export type LeadViewAction =
  | { type: "project"; project?: string; thread?: string }
  | { type: "answer"; project?: string; thread?: string; seq: number; show: LeadShow }
  | { type: "failed"; project?: string; thread?: string; seq: number; error: string };

export const LEAD_VIEW: LeadView = { projects: [], seq: 0 };

export function leadView(state: LeadView, action: LeadViewAction): LeadView {
  if (action.type === "project") {
    // Another project or another tab of it: the former answer no longer applies.
    return action.project === state.project && action.thread === state.thread && state.show !== undefined
      ? state
      : { ...state, project: action.project, thread: action.thread, show: undefined, failed: undefined };
  }
  // A check answered for a project or tab no longer looked at, or older than one applied, is dropped.
  if (action.project !== state.project || action.thread !== state.thread || action.seq <= state.seq) return state;
  if (action.type === "failed") return { ...state, seq: action.seq, failed: action.error };
  // The same answer again keeps its objects, so what was drawn from it is not redrawn.
  const same = state.show !== undefined && JSON.stringify(state.show) === JSON.stringify(action.show);
  const show = same ? state.show : action.show;
  const projects = JSON.stringify(state.projects) === JSON.stringify(action.show.projects) ? state.projects : action.show.projects;
  return { ...state, seq: action.seq, show, projects, failed: undefined };
}

/** One thing the chat draws, in order. */
/** One request the overall lead proposes for a project lead. */
export interface LeadForwardItem {
  project: string;
  text: string;
}

export type LeadVerdict = "approve" | "revise";

export type LeadItem =
  | { kind: "user"; at: string; text: string; attachments: LeadAttachment[]; provider: ProviderId; model?: string; effort?: string; origin?: string }
  /** The overall lead's proposed requests; `sent` once the person sent them on, `failed` the projects whose request did not start. */
  | { kind: "proposal"; at: string; proposalId: string; items: LeadForwardItem[]; sent: boolean; failed: string[] }
  /** `failed`: why its start failed, until it is sent again. */
  | { kind: "forward"; at: string; project: string; text: string; proposal?: string; revision?: string; failed?: string }
  | { kind: "review"; at: string; planId: string; project: string; verdict: LeadVerdict; notes: string }
  | { kind: "worker"; at: string; runId: string; cardId: string; text: string }
  | { kind: "reply"; at: string; provider: ProviderId; text: string; reportedFiles?: string[] }
  | { kind: "followup"; at: string; cardId: string; runId: string }
  /** A turn's steps and approvals, kept above its answer ("작업 n단계"). */
  | { kind: "work"; at: string; entries: LeadWorkEntry[] }
  /** `answered`: a message followed it (the picks, or something the person wrote instead). */
  | { kind: "ask"; at: string; askId: string; provider: ProviderId; intro: string; questions: AskQuestion[]; answered: boolean }
  | {
      kind: "plan";
      at: string;
      planId: string;
      title: string;
      cards: PlanCard[];
      ok: boolean;
      violations: LeadViolation[];
      /** What the check wants said but does not block on (neutral information, not an error). */
      notices: string[];
      /** What was wrong with the stored plan's shape (left out of what is drawn); a plan with any never runs from here. */
      problems: string[];
      runId?: string;
      autoRunReason?: string;
      /** The overall lead's last word on it (ADR-0036). */
      review?: { verdict: LeadVerdict; notes: string };
    }
  | { kind: "warning" | "error"; at: string; text: string };

/** Folds a thread's events into chat items; a dispatch marks its plan with the run it started. */
export function leadItems(events: readonly { t: string }[]): LeadItem[] {
  const items: LeadItem[] = [];
  // The turn's work list: one per turn, made by its first step or approval.
  let work: Extract<LeadItem, { kind: "work" }> | undefined;
  const workOf = (at: string) => {
    if (!work) { work = { kind: "work", at, entries: [] }; items.push(work); }
    return work;
  };
  const forwards: (LeadForward | undefined)[] = [];
  for (const raw of events) {
    const e = raw as LeadEvent;
    switch (e.t) {
      case "lead.step":
        if (typeof e.id === "string" && typeof e.label === "string") {
          workOf(e.at).entries.push({ type: "step", step: { id: e.id, kind: String(e.kind), label: e.label, file: e.file, added: e.added, removed: e.removed, status: String(e.status) } });
        }
        break;
      case "lead.approval":
        if (typeof e.id === "string") workOf(e.at).entries.push({ type: "approval", id: e.id, kind: e.kind, summary: String(e.summary ?? ""), files: Array.isArray(e.files) ? e.files : [] });
        break;
      case "lead.decision":
        for (const item of items) {
          if (item.kind !== "work") continue;
          for (const entry of item.entries) if (entry.type === "approval" && entry.id === e.id) Object.assign(entry, { decision: e.decision, by: e.by, reason: e.reason });
        }
        break;
      case "lead.user":
        work = undefined;
        for (const item of items) if (item.kind === "ask") item.answered = true;
        items.push({ kind: "user", at: e.at, text: e.text, attachments: e.attachments ?? [], provider: e.provider, model: e.model, effort: e.effort, ...(typeof e.origin === "string" ? { origin: e.origin } : {}) });
        break;
      case "lead.forward-plan": {
        // Stored data: requests without a project or text are left out.
        const forwards = (Array.isArray(e.items) ? e.items : []).filter((item): item is LeadForwardItem => isObject(item) && typeof item.project === "string" && typeof item.text === "string");
        if (typeof e.proposalId === "string" && forwards.length > 0) items.push({ kind: "proposal", at: e.at, proposalId: e.proposalId, items: forwards, sent: false, failed: [] });
        break;
      }
      case "lead.forward": {
        // Every lead.forward has a number, drawn or not: lead.forward-start names it by that.
        const valid = typeof e.project === "string" && typeof e.text === "string";
        const forward: LeadForward | undefined = valid
          ? { kind: "forward", at: e.at, project: e.project, text: e.text, ...(typeof e.proposal === "string" ? { proposal: e.proposal } : {}), ...(typeof e.revision === "string" ? { revision: e.revision } : {}) }
          : undefined;
        forwards.push(forward);
        if (!forward) break;
        for (const item of items) if (item.kind === "proposal" && item.proposalId === e.proposal) item.sent = true;
        items.push(forward);
        break;
      }
      case "lead.forward-start": {
        const forward = forwards[e.index];
        if (!forward) break;
        if (typeof e.error === "string") forward.failed = e.error;
        else delete forward.failed;
        break;
      }
      case "lead.review":
        if (typeof e.planId !== "string" || (e.verdict !== "approve" && e.verdict !== "revise")) break;
        for (const item of items) if (item.kind === "plan" && item.planId === e.planId) item.review = { verdict: e.verdict, notes: e.notes ?? "" };
        items.push({ kind: "review", at: e.at, planId: e.planId, project: String(e.project ?? ""), verdict: e.verdict, notes: e.notes ?? "" });
        break;
      case "lead.worker":
        work = undefined;
        items.push({ kind: "worker", at: e.at, runId: e.runId, cardId: e.cardId, text: e.text });
        break;
      case "lead.reply":
        items.push({ kind: "reply", at: e.at, provider: e.provider, text: e.text, reportedFiles: e.reportedFiles });
        break;
      case "lead.ask": {
        // A stored ask is data: questions without text or with too few options are left out.
        const questions = (Array.isArray(e.questions) ? e.questions : []).filter(
          (q): q is AskQuestion => isObject(q) && typeof q.question === "string" && Array.isArray(q.options) && q.options.length >= 2 && q.options.every((o: unknown) => isObject(o) && typeof o.label === "string"),
        );
        if (questions.length > 0) items.push({ kind: "ask", at: e.at, askId: e.askId, provider: e.provider, intro: typeof e.intro === "string" ? e.intro : "", questions, answered: false });
        break;
      }
      case "lead.plan":
        items.push(planItem(raw as Record<string, unknown>, items.length));
        break;
      case "lead.dispatch":
        for (const item of items) if (item.kind === "plan" && item.planId === e.planId) item.runId = e.runId;
        break;
      case "lead.followup":
        items.push({ kind: "followup", at: e.at, cardId: e.cardId, runId: e.runId });
        break;
      case "lead.autorun":
        for (const item of items) if (item.kind === "plan" && item.planId === e.planId && e.reason) item.autoRunReason = e.reason;
        break;
      case "lead.error":
        items.push({ kind: "error", at: e.at, text: e.text });
        break;
      case "lead.warning":
        items.push({ kind: "warning", at: e.at, text: e.text });
        break;
      default:
        // lead.start, lead.session, lead.usage, lead.seen and kinds this build does not know draw nothing.
        break;
    }
  }
  for (const item of items) {
    if (item.kind === "proposal") item.failed = forwards.flatMap((forward) => (forward?.failed !== undefined && forward.proposal === item.proposalId ? [forward.project] : []));
  }
  return items;
}

type LeadForward = Extract<LeadItem, { kind: "forward" }>;
type LeadPlan = Extract<LeadItem, { kind: "plan" }>;

/**
 * A thread's plans for the design panel: the latest, the only one offered to run, and the
 * earlier ones newest first, which stay readable (cards, dependencies, violations) but never run (#40).
 */
export function leadPlans(items: readonly LeadItem[]): { latest?: LeadPlan; earlier: LeadPlan[] } {
  const plans = items.filter((item): item is LeadPlan => item.kind === "plan");
  return { latest: plans.at(-1), earlier: plans.slice(0, -1).reverse() };
}

type LeadProposal = Extract<LeadItem, { kind: "proposal" }>;

/**
 * The overall lead's latest proposal of requests (ADR-0036), and whether it may still be sent on:
 * not yet sent or with a request that did not start (only those go again, #96), and nothing the
 * person wrote since (O refuses a stale one the same way).
 */
export function latestProposal(items: readonly LeadItem[]): { proposal?: LeadProposal; sendable: boolean } {
  const position = items.findLastIndex((item) => item.kind === "proposal");
  if (position < 0) return { sendable: false };
  const proposal = items[position] as LeadProposal;
  return { proposal, sendable: (!proposal.sent || proposal.failed.length > 0) && !items.slice(position + 1).some((item) => item.kind === "user") };
}

/** A plan the user wrote after, or the lead proposed another after, is out of date: it is not dispatched. */
export function planIsStale(items: readonly LeadItem[], planId: string): boolean {
  const position = items.findIndex((item) => item.kind === "plan" && item.planId === planId);
  return position < 0 || items.slice(position + 1).some((item) => item.kind === "user" || item.kind === "plan");
}

/** What became of an earlier plan, for its line in the earlier-designs list. */
export function earlierPlanOutcome(plan: LeadPlan): string {
  if (plan.runId) return `실행 ${plan.runId}`;
  if (plan.problems.length > 0) return "형식 오류";
  return plan.ok ? "실행하지 않음" : "정책 차단";
}

const isObject =(v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);
const text = (v: unknown): string | undefined => (typeof v === "string" ? v : undefined);

/**
 * A stored plan as the chat can draw it. The thread keeps a refused plan as the lead wrote it,
 * so its fields may be anything (`title: 42`, `cards: [null]`): every field is checked, what is
 * malformed is left out and named in `problems`, and such a plan is never offered to run.
 */
function planItem(e: Record<string, unknown>, index: number): Extract<LeadItem, { kind: "plan" }> {
  const problems: string[] = [];
  const plan = isObject(e.plan) ? e.plan : undefined;
  if (!plan) problems.push("계획이 객체가 아닙니다");
  const title = text(plan?.title)?.trim();
  if (plan && plan.title !== undefined && title === undefined) problems.push("계획 제목이 글자가 아닙니다");
  if (plan && !title) problems.push("계획 제목이 없습니다");
  const cards: PlanCard[] = [];
  if (plan && !Array.isArray(plan.cards)) problems.push("카드 목록(cards)이 배열이 아닙니다");
  (Array.isArray(plan?.cards) ? plan.cards : []).forEach((card: unknown, i: number) => {
    const id = isObject(card) ? text(card.id) : undefined;
    const provider = isObject(card) ? text(card.provider) : undefined;
    const task = isObject(card) ? text(card.task) : undefined;
    if (!isObject(card) || !id || !provider || task === undefined) {
      problems.push(`카드 ${i + 1}번의 모양이 올바르지 않아 보이지 않습니다`);
      return;
    }
    const kept: PlanCard = { id, provider, task };
    for (const key of ["role", "model", "effort"] as const) {
      const value = text(card[key]);
      if (value) kept[key] = value;
    }
    if (card.depends_on !== undefined) {
      if (!Array.isArray(card.depends_on) || card.depends_on.some((id: unknown) => typeof id !== "string")) problems.push(`작업 ${id}의 선행 작업이 올바르지 않습니다`);
      else kept.depends_on = card.depends_on;
    }
    if (card.resources !== undefined) {
      if (!Array.isArray(card.resources) || card.resources.length > 64 || card.resources.some((r: unknown) => !isObject(r) || (r.kind !== "project-path" && r.kind !== "external") || typeof r.key !== "string" || !r.key.trim() || new TextEncoder().encode(r.key).length > 1024 || /[\u0000-\u001f\u007f-\u009f]/u.test(r.key) || (r.mode !== "read" && r.mode !== "write") || Object.keys(r).some((k) => !["kind", "key", "mode"].includes(k)))) problems.push(`작업 ${id}의 자원 선언이 올바르지 않습니다`);
      else kept.resources = card.resources;
    }
    cards.push(kept);
  });
  const violations = (Array.isArray(e.violations) ? e.violations : []).flatMap((v: unknown): LeadViolation[] => {
    if (!isObject(v) || typeof v.rule !== "string" || typeof v.message !== "string") return [];
    return [typeof v.cardId === "string" ? { rule: v.rule, message: v.message, cardId: v.cardId } : { rule: v.rule, message: v.message }];
  });
  return {
    kind: "plan",
    at: text(e.at) ?? "",
    planId: text(e.planId) ?? `plan-${index}`,
    title: title || "제목 없는 계획",
    cards,
    ok: e.ok === true && problems.length === 0,
    violations,
    notices: (Array.isArray(e.notices) ? e.notices : []).filter((n: unknown): n is string => typeof n === "string" && n.length > 0),
    problems,
  };
}

/** The agents that can lead and the efforts each takes (crates/o-core/src/providers `lead_efforts`). */
export const LEAD_EFFORTS = {
  claude: ["low", "medium", "high", "xhigh", "max"],
  codex: ["low", "medium", "high", "xhigh"],
  agy: ["low", "medium", "high", "xhigh", "max"],
} as const satisfies Partial<Record<ProviderId, readonly string[]>>;

export type LeadProvider = keyof typeof LEAD_EFFORTS;
export const LEAD_PROVIDERS = Object.keys(LEAD_EFFORTS) as LeadProvider[];

export const LEAD_NAME: Record<LeadProvider, string> = { claude: "Claude Code", codex: "Codex", agy: "Antigravity" };

export function isLeadProvider(id: string): id is LeadProvider {
  return id in LEAD_EFFORTS;
}

/**
 * Whether an agent can lead now: an agent that leads, installed, and not known to be signed
 * out (a CLI that cannot tell, like agy without its quota, may still be tried: it says so
 * if it is not signed in). Onboarding's "시작하기" and the composer use this one rule.
 */
export function canLead(agent: { id: string; installed: boolean; loggedIn: boolean | null }): boolean {
  return agent.installed && agent.loggedIn !== false && isLeadProvider(agent.id);
}

/** One model a lead may run with (`o lead models`): the id passed as `--model`, and its name. */
export interface LeadModel {
  id: string;
  label: string;
  /** For an alias (`id`), the model it stands for now (Claude: `opus` → `claude-opus-5-5`). */
  hint?: string;
}

/** The model id shape a card's `model` takes and `o lead send --model` checks (crates/o-core/src/policy.rs `MODEL`). */
export const MODEL_ID = /^[A-Za-z0-9][A-Za-z0-9._:\-/[\]]{0,79}$/;

/** The name shown for a model: its listed label, else the id itself (a typed-in one); none for the CLI's default. */
export function modelLabel(models: readonly LeadModel[], id: string | undefined): string | undefined {
  if (!id) return undefined;
  return models.find((m) => m.id === id)?.label ?? id;
}

export const EFFORT_WORD: Record<string, string> = { minimal: "최소", low: "낮음", medium: "보통", high: "높음", xhigh: "매우 높음", max: "최대" };

/** The effort kept when the agent changes: the same one if the new agent takes it, else the nearest below, else its lowest. */
export function effortFor(provider: LeadProvider, wanted: string | undefined): string {
  const efforts: readonly string[] = LEAD_EFFORTS[provider];
  if (wanted && efforts.includes(wanted)) return wanted;
  const ladder = ["minimal", "low", "medium", "high", "xhigh", "max"];
  const at = ladder.indexOf(wanted ?? "medium");
  for (let i = at; i >= 0; i--) if (efforts.includes(ladder[i])) return ladder[i];
  return efforts[0];
}

/** What O accepts as an attachment (crates/o-core/src/lead/files.rs). */
export const ATTACH = {
  maxBytes: 10 * 1024 * 1024,
  perMessage: 5,
  images: ["png", "jpg", "jpeg", "gif", "webp"],
  documents: ["pdf", "md", "txt", "csv", "json", "log"],
} as const;

/** The file picker's filter. */
export const ATTACH_ACCEPT = [...ATTACH.images, ...ATTACH.documents].map((e) => `.${e}`).join(",");

/** Why a file cannot be attached, before it is sent; undefined when it can (O checks again). */
export function attachProblem(name: string, size: number, already: number): string | undefined {
  if (already >= ATTACH.perMessage) return `첨부는 메시지당 ${ATTACH.perMessage}개까지입니다`;
  const ext = name.includes(".") ? name.split(".").pop()!.toLowerCase() : "";
  if (![...ATTACH.images, ...ATTACH.documents].includes(ext as never)) return `${name}: 이미지(png·jpg·gif·webp)나 문서(pdf·md·txt·csv·json·log)만 첨부할 수 있습니다`;
  if (size > ATTACH.maxBytes) return `${name}: 파일 하나에 10MB까지입니다`;
  if (size === 0) return `${name}: 빈 파일입니다`;
  return undefined;
}

/** "1.2 MB", "34 KB", "900 B". */
export function fileSize(bytes: number): string {
  if (bytes >= 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
  if (bytes >= 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${bytes} B`;
}

/** One file in the composer: uploading, saved for the thread, or refused. */
export interface DraftFile {
  key: string;
  name: string;
  size: number;
  image: boolean;
  saved?: LeadAttachment;
  error?: string;
}

/**
 * The composer's message being written for one project: its text and files, and a generation
 * that moves on whenever the draft is sent, so an upload started before cannot land in it.
 */
export interface Draft {
  text: string;
  files: DraftFile[];
  gen: number;
}

/** Every project's draft, by `draftKey`. */
export type Drafts = Readonly<Record<string, Draft>>;

export const EMPTY_DRAFT: Draft = { text: "", files: [], gen: 0 };

/** A draft's key: the folder as O compares it, and the conversation when the project has several (ADR-0034). */
export function draftKey(project: string, thread?: string): string {
  const folder = project.replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase();
  return thread ? `${folder}#${thread}` : folder;
}

export function draftOf(drafts: Drafts, key: string): Draft {
  return drafts[key] ?? EMPTY_DRAFT;
}

export function editDraft(drafts: Drafts, key: string, change: (draft: Draft) => Partial<Pick<Draft, "text" | "files">>): Drafts {
  const draft = draftOf(drafts, key);
  return { ...drafts, [key]: { ...draft, ...change(draft) } };
}

/**
 * An upload finished: it lands in the draft it was started from, only if that draft has not
 * been sent since (same generation) and still has its chip. Another project's draft never gets it.
 * A draft started before the project's first conversation was named has moved to that
 * conversation's key meanwhile: the answer follows its chip there (#88).
 */
export function uploadDone(drafts: Drafts, key: string, gen: number, fileKey: string, result: { saved?: LeadAttachment; error?: string }): Drafts {
  const holds = (at: string) => drafts[at]?.files.some((f) => f.key === fileKey) ?? false;
  const at = drafts[key]?.gen === gen && holds(key) ? key : movedChip(drafts, key, holds);
  if (at === undefined) return drafts;
  return editDraft(drafts, at, (d) => ({ files: d.files.map((f) => (f.key === fileKey ? { ...f, ...result, image: result.saved?.image ?? f.image } : f)) }));
}

/**
 * Where a chip of the project's own draft (a key without a conversation) is now: one conversation
 * of that same project, when it still has the chip. A removed chip, a sent draft, another project
 * and a draft that belonged to a conversation from the start give nothing.
 */
function movedChip(drafts: Drafts, key: string, holds: (at: string) => boolean): string | undefined {
  if (key.includes("#")) return undefined;
  return Object.keys(drafts).find((at) => at.startsWith(`${key}#`) && !at.includes("#", key.length + 1) && holds(at));
}

/** The draft taken to send: emptied, and its generation moved on. */
export function takeDraft(drafts: Drafts, key: string): { drafts: Drafts; draft: Draft } {
  const draft = draftOf(drafts, key);
  return { drafts: { ...drafts, [key]: { text: "", files: [], gen: draft.gen + 1 } }, draft };
}

/** A send that failed (nothing recorded): its text and files come back, unless a new draft was started meanwhile. */
export function restoreDraft(drafts: Drafts, key: string, taken: Draft): Drafts {
  return editDraft(drafts, key, (now) => ({ text: now.text || taken.text, files: now.files.length ? now.files : taken.files }));
}

/** Where the composer is: a project folder (in any spelling) and, once named, its conversation. */
export interface DraftPlace {
  project: string | undefined;
  thread: string | undefined;
}

/**
 * The composer went from one place to another. A draft typed before the answer named the
 * project's conversation moves to that conversation's key — only within the same project, and
 * only into an empty draft: another project's draft is never carried along, and a conversation
 * that already has a draft keeps it while the project's own stays where it was typed (#86).
 * Drafts between open tabs stay where they were typed.
 */
export function settleDraft(drafts: Drafts, from: DraftPlace, to: DraftPlace): Drafts {
  if (!from.project || !to.project || from.thread || !to.thread) return drafts;
  const source = draftKey(from.project);
  if (draftKey(to.project) !== source) return drafts;
  const typed = draftOf(drafts, source);
  if (!typed.text && !typed.files.length) return drafts;
  const target = draftKey(to.project, to.thread);
  const there = draftOf(drafts, target);
  if (there.text || there.files.length) return drafts;
  return restoreDraft(takeDraft(drafts, source).drafts, target, typed);
}

/**
 * Why the lead thread could not be read, for the chat: the error, and when it looks like an
 * `o` that does not know `lead` (it printed its usage, or the command failed outright), what to check.
 */
export function leadReadProblem(error: string): string {
  const oldCli = /usage:|unknown command|unrecognized|Command failed|o exited with/i.test(error);
  return oldCli ? `${error.trim()}\n\nO CLI가 총괄 명령(o lead)을 모르는 옛 버전일 수 있습니다. 앱을 업데이트하거나 O CLI를 다시 설치한 뒤 다시 시도하세요.` : error.trim();
}

/** A card task too long for the plan table's three lines: it gets a "전체 보기". */
export function taskIsLong(task: string): boolean {
  return task.length > 120 || task.split("\n").length > 3;
}

/**
 * How a plan will run, before "실행": its cards in stages by depends_on (a stage starts when the one
 * before it ends), and the cards that declare no resources and so run alone (ADR-0037). Unknown or
 * circular dependencies end up in a last stage; the runner fails those cards. Slots are not counted.
 */
export function planOrder(cards: readonly Pick<PlanCard, "id" | "depends_on" | "resources">[]): { stages: string[][]; alone: string[] } {
  const placed = new Set<string>();
  const stages: string[][] = [];
  let left = [...cards];
  while (left.length > 0) {
    const ready = left.filter((card) => (card.depends_on ?? []).every((id) => placed.has(id)));
    const stage = ready.length > 0 ? ready : left;
    stages.push(stage.map((card) => card.id));
    for (const card of stage) placed.add(card.id);
    left = left.filter((card) => !placed.has(card.id));
  }
  const alone = cards.length > 1 ? cards.filter((card) => !card.resources?.length).map((card) => card.id) : [];
  return { stages, alone };
}

/** One line for the plan table: "모두 동시에" or the stages in order, and who runs alone. */
export function planOrderText(cards: readonly Pick<PlanCard, "id" | "depends_on" | "resources">[]): string {
  const { stages, alone } = planOrder(cards);
  const order = stages.length <= 1 ? `${cards.length}개 작업 모두 동시에 시작` : `${stages.length}단계로 차례 실행: ${stages.map((stage) => stage.join(", ")).join(" → ")}`;
  return alone.length > 0 ? `${order} · 자원 미선언(${alone.join(", ")})은 혼자 실행` : order;
}

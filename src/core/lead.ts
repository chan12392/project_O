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

export interface PlanCard {
  id: string;
  provider: string;
  role?: string;
  model?: string;
  effort?: string;
  task: string;
  depends_on?: string[];
}

export type LeadEvent =
  | { t: "lead.start"; at: string; threadId: string; project: string }
  | { t: "lead.user"; at: string; text: string; attachments?: LeadAttachment[]; provider: ProviderId; model?: string; effort?: string }
  | { t: "lead.session"; at: string; provider: ProviderId; sessionId: string }
  | { t: "lead.reply"; at: string; provider: ProviderId; text: string }
  | { t: "lead.design"; at: string; design: { goal: string; decisions: string[]; questions: string[] } }
  | { t: "lead.plan"; at: string; planId: string; plan: { title?: string; project?: string; cards?: PlanCard[] }; ok: boolean; violations: LeadViolation[] }
  | { t: "lead.dispatch"; at: string; planId: string; runId: string; clean: boolean }
  | { t: "lead.autorun"; at: string; planId: string; reason?: string | null }
  | { t: "lead.usage"; at: string; provider: ProviderId; input: number; cached: number; output: number }
  | { t: "lead.error"; at: string; text: string }
  /** A CLI session got the turn whose lead.user is event `through`; the chat draws nothing for it. */
  | { t: "lead.seen"; at: string; provider: ProviderId; sessionId: string; through: number };

/** What `o lead show --json` prints. `busy` is the answering agent, or "starting" for a turn still setting up. */
export interface LeadShow {
  thread: { threadId: string; project: string; busy: ProviderId | "starting" | null; events: LeadEvent[] } | null;
  projects: { project: string; threadId: string }[];
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
  show?: LeadShow;
  projects: LeadShow["projects"];
  failed?: string;
  seq: number;
}

export type LeadViewAction =
  | { type: "project"; project?: string }
  | { type: "answer"; project?: string; seq: number; show: LeadShow }
  | { type: "failed"; project?: string; seq: number; error: string };

export const LEAD_VIEW: LeadView = { projects: [], seq: 0 };

export function leadView(state: LeadView, action: LeadViewAction): LeadView {
  if (action.type === "project") {
    return action.project === state.project && state.show !== undefined ? state : { ...state, project: action.project, show: undefined, failed: undefined };
  }
  if (action.project !== state.project || action.seq <= state.seq) return state;
  if (action.type === "failed") return { ...state, seq: action.seq, failed: action.error };
  // The same answer again keeps its objects, so what was drawn from it is not redrawn.
  const same = state.show !== undefined && JSON.stringify(state.show) === JSON.stringify(action.show);
  const show = same ? state.show : action.show;
  const projects = JSON.stringify(state.projects) === JSON.stringify(action.show.projects) ? state.projects : action.show.projects;
  return { ...state, seq: action.seq, show, projects, failed: undefined };
}

/** One thing the chat draws, in order. */
export type LeadItem =
  | { kind: "user"; at: string; text: string; attachments: LeadAttachment[]; provider: ProviderId; model?: string; effort?: string }
  | { kind: "reply"; at: string; provider: ProviderId; text: string }
  | {
      kind: "plan";
      at: string;
      planId: string;
      title: string;
      cards: PlanCard[];
      ok: boolean;
      violations: LeadViolation[];
      /** What was wrong with the stored plan's shape (left out of what is drawn); a plan with any never runs from here. */
      problems: string[];
      runId?: string;
      autoRunReason?: string;
    }
  | { kind: "error"; at: string; text: string };

/** Folds a thread's events into chat items; a dispatch marks its plan with the run it started. */
export function leadItems(events: readonly { t: string }[]): LeadItem[] {
  const items: LeadItem[] = [];
  for (const raw of events) {
    const e = raw as LeadEvent;
    switch (e.t) {
      case "lead.user":
        items.push({ kind: "user", at: e.at, text: e.text, attachments: e.attachments ?? [], provider: e.provider, model: e.model, effort: e.effort });
        break;
      case "lead.reply":
        items.push({ kind: "reply", at: e.at, provider: e.provider, text: e.text });
        break;
      case "lead.plan":
        items.push(planItem(raw as Record<string, unknown>, items.length));
        break;
      case "lead.dispatch":
        for (const item of items) if (item.kind === "plan" && item.planId === e.planId) item.runId = e.runId;
        break;
      case "lead.autorun":
        for (const item of items) if (item.kind === "plan" && item.planId === e.planId && e.reason) item.autoRunReason = e.reason;
        break;
      case "lead.error":
        items.push({ kind: "error", at: e.at, text: e.text });
        break;
      default:
        // lead.start, lead.session, lead.usage, lead.seen and kinds this build does not know draw nothing.
        break;
    }
  }
  return items;
}

const isObject = (v: unknown): v is Record<string, unknown> => typeof v === "object" && v !== null && !Array.isArray(v);
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
    problems,
  };
}

/** The agents that can lead and the efforts each takes (crates/o-core/src/providers `lead_efforts`). */
export const LEAD_EFFORTS = {
  claude: ["low", "medium", "high", "xhigh", "max"],
  codex: ["minimal", "low", "medium", "high", "xhigh"],
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

/** A project's draft key: folders compare as O compares them (any slash, no trailing one, any case). */
export function draftKey(project: string): string {
  return project.replace(/\\/g, "/").replace(/\/+$/, "").toLowerCase();
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
 */
export function uploadDone(drafts: Drafts, key: string, gen: number, fileKey: string, result: { saved?: LeadAttachment; error?: string }): Drafts {
  const draft = drafts[key];
  if (!draft || draft.gen !== gen || !draft.files.some((f) => f.key === fileKey)) return drafts;
  return editDraft(drafts, key, (d) => ({ files: d.files.map((f) => (f.key === fileKey ? { ...f, ...result, image: result.saved?.image ?? f.image } : f)) }));
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

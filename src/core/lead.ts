/**
 * The lead chat (ADR-0008) as screens see it: the thread events `o lead show --json`
 * prints (crates/o-core/src/lead), folded into what the chat draws, and the choices
 * the composer offers. Lead events are their own small type, not run events.
 */
import type { ProviderId } from "./plan.ts";

export interface LeadAttachment {
  name: string;
  path: string;
  size: number;
  image: boolean;
}

export interface LeadViolation {
  rule: string;
  message: string;
  cardId?: string;
}

export interface PlanCard {
  id: string;
  provider: string;
  role?: string;
  model?: string;
  effort?: string;
  task: string;
}

export type LeadEvent =
  | { t: "lead.start"; at: string; threadId: string; project: string }
  | { t: "lead.user"; at: string; text: string; attachments?: LeadAttachment[]; provider: ProviderId; model?: string; effort?: string }
  | { t: "lead.session"; at: string; provider: ProviderId; sessionId: string }
  | { t: "lead.reply"; at: string; provider: ProviderId; text: string }
  | { t: "lead.plan"; at: string; planId: string; plan: { title?: string; project?: string; cards?: PlanCard[] }; ok: boolean; violations: LeadViolation[] }
  | { t: "lead.dispatch"; at: string; planId: string; runId: string; clean: boolean }
  | { t: "lead.usage"; at: string; provider: ProviderId; input: number; cached: number; output: number }
  | { t: "lead.error"; at: string; text: string }
  /** A CLI session got the turn whose lead.user is event `through`; the chat draws nothing for it. */
  | { t: "lead.seen"; at: string; provider: ProviderId; sessionId: string; through: number };

/** What `o lead show --json` prints. `busy` is the answering agent, or "starting" for a turn still setting up. */
export interface LeadShow {
  thread: { threadId: string; project: string; busy: ProviderId | "starting" | null; events: LeadEvent[] } | null;
  projects: { project: string; threadId: string }[];
}

/** One thing the chat draws, in order. */
export type LeadItem =
  | { kind: "user"; at: string; text: string; attachments: LeadAttachment[]; provider: ProviderId; model?: string; effort?: string }
  | { kind: "reply"; at: string; provider: ProviderId; text: string }
  | { kind: "plan"; at: string; planId: string; title: string; cards: PlanCard[]; ok: boolean; violations: LeadViolation[]; runId?: string }
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
        items.push({
          kind: "plan",
          at: e.at,
          planId: e.planId,
          title: e.plan?.title?.trim() || "제목 없는 계획",
          cards: Array.isArray(e.plan?.cards) ? e.plan.cards : [],
          ok: e.ok,
          violations: e.violations ?? [],
        });
        break;
      case "lead.dispatch":
        for (const item of items) if (item.kind === "plan" && item.planId === e.planId) item.runId = e.runId;
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

/** One model a lead may run with (`o lead models`): the id passed as `--model`, and its name. */
export interface LeadModel {
  id: string;
  label: string;
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

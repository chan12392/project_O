import type { CardState, RunView } from "../../../src/core/snapshot.ts";
import type { ProviderId } from "../../../src/core/plan.ts";

const clock = new Intl.DateTimeFormat("ko-KR", { hour: "2-digit", minute: "2-digit", hour12: false });

export function timeOf(iso: string | undefined): string {
  return iso ? clock.format(new Date(iso)) : "";
}

export const PROVIDER_NAME: Record<ProviderId, string> = { claude: "Claude", codex: "Codex", agy: "Antigravity", opencode: "OpenCode" };
export const PROVIDER_SHORT: Record<ProviderId, string> = { claude: "Claude", codex: "Codex", agy: "Agy", opencode: "OpenCode" };

/** Display names for team parties: a card's label and agent, "총괄" for the lead, "모두" for all. */
export function names(run: RunView): (party: string) => string {
  const byId = new Map(run.cards.map((c) => [c.id, `${c.label} (${PROVIDER_SHORT[c.provider]})`]));
  return (party) => (party === "lead" ? "총괄" : party === "all" ? "모두" : byId.get(party) ?? party);
}

export const STATE_WORD: Record<CardState, string> = {
  waiting: "대기",
  queued: "차례 기다림",
  running: "작업 중",
  done: "완료",
  failed: "실패",
  stopped: "중지됨",
};

/**
 * A glance, not the record: cut by characters rather than by a CSS line clamp,
 * which leaves hidden lines painted under their box. The card log keeps the full text.
 */
export function clip(text: string, max: number): string {
  return text.length > max ? `${text.slice(0, max - 1).trimEnd()}…` : text;
}

/** The model and effort a card asked for, or the CLI's own default. */
export function modelLine(model: string | undefined, effort: string | undefined): string {
  if (!model && !effort) return "기본 모델";
  return [model, effort].filter(Boolean).join(" ");
}

/** Token counts and the like, short: 12.3만. */
export const compact = new Intl.NumberFormat("ko-KR", { notation: "compact", maximumFractionDigits: 1 });

/** A square, borderless icon button in a toolbar or header. */
export const ICON_BUTTON = "flex size-7 items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground focus-ring";

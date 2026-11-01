import type { CardState } from "../../src/core/snapshot.ts";
import type { ProviderId } from "../../src/core/plan.ts";

const clock = new Intl.DateTimeFormat("ko-KR", { hour: "2-digit", minute: "2-digit", hour12: false });

export function timeOf(iso: string | undefined): string {
  return iso ? clock.format(new Date(iso)) : "";
}

export const PROVIDER_NAME: Record<ProviderId, string> = { claude: "Claude", codex: "Codex", agy: "Antigravity" };
export const PROVIDER_SHORT: Record<ProviderId, string> = { claude: "Claude", codex: "Codex", agy: "Agy" };

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

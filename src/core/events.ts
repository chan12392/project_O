/**
 * The normalized event stream every run writes to `events.jsonl`.
 *
 * This is the contract between O CLI and any screen (chat UI, character pane,
 * Control board). Provider-specific stream formats never leave
 * `providers.ts`; screens only ever see these shapes.
 */
import type { ProviderId } from "./plan.ts";

export type CardStatus = "done" | "failed" | "stopped";

export interface CardInfo {
  id: string;
  provider: ProviderId;
  role: string;
  model?: string;
  effort?: string;
}

export type OEvent =
  | { t: "run.start"; at: string; runId: string; title: string; project: string; cards: CardInfo[] }
  | { t: "card.queued"; at: string; cardId: string; reason: string }
  | { t: "card.start"; at: string; cardId: string; pid: number | undefined }
  | { t: "card.session"; at: string; cardId: string; sessionId: string }
  | { t: "card.message"; at: string; cardId: string; text: string }
  | { t: "card.tool"; at: string; cardId: string; name: string; summary: string }
  | { t: "card.limit"; at: string; cardId: string; until: string }
  | { t: "card.end"; at: string; cardId: string; status: CardStatus; result?: string; exitCode: number | null }
  | { t: "run.end"; at: string; runId: string; status: "done" | "failed" | "stopped" };

/** What a provider stream line means, before it gets a timestamp and card id. */
export type StreamSignal =
  | { kind: "session"; sessionId: string }
  | { kind: "message"; text: string }
  | { kind: "tool"; name: string; summary: string }
  | { kind: "limit"; until: string }
  | { kind: "final"; ok: boolean; text: string };

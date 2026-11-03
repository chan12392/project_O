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
  /** What people call the card: its role label, or the start of its task. */
  label: string;
  role?: string;
  model?: string;
  effort?: string;
}

export type OEvent =
  | { t: "run.start"; at: string; runId: string; title: string; project: string; cards: CardInfo[] }
  | { t: "run.followup"; at: string; sourceRunId: string; cardId: string }
  | { t: "card.queued"; at: string; cardId: string; reason: string }
  | { t: "card.start"; at: string; cardId: string; pid: number | undefined }
  | { t: "card.session"; at: string; cardId: string; sessionId: string }
  | { t: "card.message"; at: string; cardId: string; text: string }
  | { t: "card.input"; at: string; cardId: string; text: string }
  | { t: "card.tool"; at: string; cardId: string; name: string; summary: string }
  | { t: "card.limit"; at: string; cardId: string; until: string }
  | { t: "card.end"; at: string; cardId: string; status: CardStatus; result?: string; exitCode: number | null }
  /** A team message between agents of one run (`o msg send`), "lead" for the supervisor, "all" for everyone. */
  | { t: "msg"; at: string; from: string; to: string; text: string; id?: string; kind?: "message" | "question" }
  | { t: "msg.state"; at: string; id: string; recipient: string; state: "stored" | "delivered" | "unknown" | "acknowledged" | "cancelled" }
  /** Tokens a card spent since its last usage event (fresh input, cached input, output). */
  | { t: "card.usage"; at: string; cardId: string; input: number; cached: number; output: number }
  /** A subscription window the provider reported: share used (0 to 1) and when it resets. */
  | { t: "quota"; at: string; provider: ProviderId; window: string; used: number; resetsAt: string }
  | { t: "run.end"; at: string; runId: string; status: CardStatus }
  /** `o review --post` commented the review card's answer on the PR (ADR-0006). */
  | { t: "review.posted"; at: string; pr: number; url: string; head?: string }
  /**
   * The runner lowered a card's effort before launching it to keep its provider within today's
   * usage budget (ADR-0004). `spent` and `allowance` are percent points of the weekly window.
   */
  | { t: "card.budget"; at: string; cardId: string; provider: ProviderId; effortFrom?: string; effortTo: string; spent: number; allowance: number }
  /** Cards whose prompt got a shared context section (ADR-0005), and its characters in all. */
  | { t: "run.context"; at: string; runId: string; cards: number; chars: number; cardIds: string[] }
  /**
   * The runner's own check right before the start (`o _run`) refused the plan: no card started.
   * The violations are the ones `o check` would print; `run.end failed` follows.
   */
  | { t: "run.blocked"; at: string; runId: string; violations: Violation[] }
  /** The profile the run's cards used (project_O 프로필 사용, ADR-0013) and whether the shared context was on. */
  | { t: "run.mode"; at: string; runId: string; profile: "project-o" | "own"; context: boolean; yolo?: boolean };

/** One policy refusal, as crates/o-core/src/policy.rs `Violation` serializes it. */
export interface Violation {
  rule: string;
  message: string;
  cardId?: string;
}

/** What a provider stream line means, before it gets a timestamp and card id. */
export type StreamSignal =
  | { kind: "session"; sessionId: string }
  | { kind: "message"; text: string }
  | { kind: "tool"; name: string; summary: string }
  | { kind: "limit"; until: string }
  | { kind: "usage"; input: number; cached: number; output: number }
  | { kind: "quota"; window: string; used: number; resetsAt: string }
  | { kind: "final"; ok: boolean; text: string };

/**
 * A plan is what the supervising agent hands to `o dispatch`: one project and
 * a few work cards, each pinned to a provider CLI.
 *
 * Cards carry only data. They cannot name a binary, arguments, environment or
 * credentials — `providers.ts` alone decides how a provider is launched, and
 * `policy.ts` rejects any field outside CARD_FIELDS.
 */
export const PROVIDERS = ["claude", "codex", "agy"] as const;
export type ProviderId = (typeof PROVIDERS)[number];

export const CARD_FIELDS = ["id", "provider", "role", "task", "model", "effort"] as const;
export const PLAN_FIELDS = ["title", "project", "cards"] as const;

export interface Card {
  id: string;
  provider: ProviderId;
  task: string;
  /** A free label for people ("코딩", "검토"). It never selects or limits a model. */
  role?: string;
  model?: string;
  effort?: string;
}

/** What screens and prompts call a card: its role label if it has one, else its id. */
export function cardLabel(card: { id: string; role?: string }): string {
  return card.role?.trim() || card.id;
}

export interface Plan {
  title: string;
  project: string;
  cards: Card[];
}

export function isProvider(value: unknown): value is ProviderId {
  return typeof value === "string" && (PROVIDERS as readonly string[]).includes(value);
}

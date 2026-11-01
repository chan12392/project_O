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
  role: string;
  task: string;
  model?: string;
  effort?: string;
}

export interface Plan {
  title: string;
  project: string;
  cards: Card[];
}

export function isProvider(value: unknown): value is ProviderId {
  return typeof value === "string" && (PROVIDERS as readonly string[]).includes(value);
}

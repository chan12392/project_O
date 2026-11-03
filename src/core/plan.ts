/**
 * The screen's view of a plan's cards. Plans themselves are checked and run
 * by the O CLI (crates/o-core); the app only reads the events they produce.
 */
export const PROVIDERS = ["claude", "codex", "agy", "opencode", "devin"] as const;
export type ProviderId = (typeof PROVIDERS)[number];

/** What screens and prompts call a card: its role label if it has one, else its id. */
export function cardLabel(card: { id: string; role?: string }): string {
  return card.role?.trim() || card.id;
}

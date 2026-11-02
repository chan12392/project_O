import type { ProviderId } from "./plan.ts";

/**
 * The dispatch caps, for the screen to show next to today's counts. The O CLI
 * enforces them in Rust (crates/o-core/src/policy.rs); tests/limits.test.ts
 * fails if these drift from there.
 */
export const LIMITS = {
  maxActiveTotal: 6,
  maxActivePerProvider: { claude: 3, codex: 3, agy: 2, opencode: 2, devin: 2 } as Record<ProviderId, number>,
  maxCardsPerDay: 60,
} as const;

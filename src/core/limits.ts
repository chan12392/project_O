import type { ProviderId } from "./plan.ts";

/**
 * The concurrency caps shown on screen. The O CLI
 * enforces them in Rust (crates/o-core/src/policy.rs); tests/limits.test.ts
 * fails if these drift from there.
 */
export const LIMITS = {
  maxActiveTotal: 6,
  maxActivePerProvider: { claude: 3, codex: 3, agy: 2, opencode: 2, devin: 2 } as Record<ProviderId, number>,
} as const;

import type { ProviderId } from "./plan.ts";

/**
 * A subscription window as `o usage` reports it (crates/o-core reads Codex
 * through its app-server and Antigravity through `agy -p /usage`); Claude's
 * come from its run streams as `quota` events.
 */
export interface UsageWindow {
  provider: ProviderId;
  /** A stable id: five_hour, seven_day, or the CLI's own bucket id (gemini-weekly). */
  window: string;
  /** The model group a window covers, when the CLI splits by group (agy). */
  group?: string;
  /** Share used, 0 to 1. */
  used: number;
  resetsAt: string;
}

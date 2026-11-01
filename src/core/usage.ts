import type { ProviderId } from "./plan.ts";

/**
 * Subscription windows read through each official CLI, the way Orca reads them:
 * Codex answers `account/rateLimits/read` on a short-lived read-only `codex app-server`,
 * Antigravity answers `agy -p /usage --output-format json` without spending a turn.
 * Claude reports its windows inside every run's stream (see providers.ts).
 * Pure parsers; nothing here touches credentials or provider backends.
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

function windowName(minutes: unknown): string {
  if (minutes === 300) return "five_hour";
  if (minutes === 10080) return "seven_day";
  return typeof minutes === "number" ? `${minutes}m` : "window";
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** The `account/rateLimits/read` result: primary and secondary windows of the codex limit. */
export function parseCodexRateLimits(result: unknown): UsageWindow[] {
  const limits = isRecord(result) && isRecord(result.rateLimits) ? result.rateLimits : undefined;
  if (!limits) return [];
  return [limits.primary, limits.secondary].flatMap((w) =>
    isRecord(w) && typeof w.usedPercent === "number" && typeof w.resetsAt === "number"
      ? [{ provider: "codex" as const, window: windowName(w.windowDurationMins), used: w.usedPercent / 100, resetsAt: new Date(w.resetsAt * 1000).toISOString() }]
      : [],
  );
}

/** The `agy -p /usage` JSON envelope: groups of buckets with a remaining fraction each. */
export function parseAgyUsage(stdout: string): UsageWindow[] {
  let data: unknown;
  try {
    data = JSON.parse(stdout);
  } catch {
    return [];
  }
  const groups = isRecord(data) && isRecord(data.command) && isRecord(data.command.data) && Array.isArray(data.command.data.groups) ? data.command.data.groups : [];
  return groups.flatMap((group) => {
    if (!isRecord(group) || !Array.isArray(group.buckets)) return [];
    return group.buckets.flatMap((b) =>
      isRecord(b) && typeof b.remaining_fraction === "number" && typeof b.reset_time === "string" && b.disabled !== true
        ? [
            {
              provider: "agy" as const,
              window: b.window === "5h" ? "five_hour" : b.window === "weekly" ? "seven_day" : String(b.id ?? "window"),
              group: typeof group.name === "string" ? group.name : undefined,
              used: Math.max(0, Math.min(1, 1 - b.remaining_fraction)),
              resetsAt: b.reset_time,
            },
          ]
        : [],
    );
  });
}

import type { ProviderId } from "./plan.ts";

export interface UsageSettlement {
  usageId: string;
  revision: number;
  provider: ProviderId;
  profile: string;
  sessionId: string | null;
  occurredAt: string;
  endedAt: string;
  sourceStart: number;
  sourceEnd: number;
  resumed: boolean;
  appServer: boolean;
  input: number;
  cached: number;
  output: number;
  reported?: boolean;
  complete: boolean;
  scope: string;
  reason: string | null;
  models: string[];
  cumulative: [number, number, number] | null;
  replaces: (string | number)[];
  source: string;
  version: number;
}

export function legacyReferences(events: readonly { t: string; at: string; cardId?: string; provider?: string }[]): (string | undefined)[] {
  const seen = new Map<string, number>();
  return events.map((e) => {
    if (e.t !== "card.usage" && e.t !== "lead.usage") return undefined;
    const key = JSON.stringify([e.t, e.at, e.cardId ?? null, e.provider ?? null]);
    const ordinal = seen.get(key) ?? 0;
    seen.set(key, ordinal + 1);
    return `legacy:${key}:${ordinal}`;
  });
}

/** The latest authoritative value replaces the referenced deltas. Keep raw events for evidence. */
export function projectUsage<T extends { t: string; at: string }>(events: readonly T[]): T[] {
  const references = new Map(legacyReferences(events).flatMap((key, index) => key === undefined ? [] : [[key, index] as const]));
  const latest = new Map<string, { index: number; revision: number }>();
  const conflicts = new Set<string>();
  for (const [index, event] of events.entries()) {
    if (event.t !== "card.usage.settled" && event.t !== "lead.usage.settled") continue;
    const e = event as T & UsageSettlement;
    if (!e.usageId || !Number.isSafeInteger(e.revision) || e.revision < 1 || e.version !== 1 || [e.input, e.cached, e.output].some((n) => !Number.isFinite(n) || n < 0)) continue;
    const old = latest.get(e.usageId);
    if (!old || old.revision < e.revision) { latest.set(e.usageId, { index, revision: e.revision }); conflicts.delete(e.usageId); }
    else if (old.revision === e.revision) {
      const previous = events[old.index] as T & UsageSettlement;
      const oldPayload = Object.entries(previous).filter(([key]) => key !== "at").sort(([a], [b]) => a.localeCompare(b));
      const newPayload = Object.entries(e).filter(([key]) => key !== "at").sort(([a], [b]) => a.localeCompare(b));
      if (JSON.stringify(oldPayload) !== JSON.stringify(newPayload)) conflicts.add(e.usageId);
    }
  }
  const replaced = new Set<number>(), partial = new Set<number>();
  for (const { index } of latest.values()) {
    const e = events[index] as T & UsageSettlement & { cardId?: string };
    for (const key of e.replaces ?? []) {
      const ref = typeof key === "string" ? references.get(key) : key;
      if (ref === undefined || !Number.isSafeInteger(ref) || ref < 0 || ref >= index) continue;
      const old = events[ref] as T & { cardId?: string; provider?: string };
      if (e.t === "card.usage.settled" ? old.t === "card.usage" && old.cardId === e.cardId : old.t === "lead.usage" && old.provider === e.provider) (e.reported === false ? partial : replaced).add(ref);
    }
  }
  return events.flatMap((event, index) => {
    if (replaced.has(index) && (event.t === "card.usage" || event.t === "lead.usage")) return [];
    if (event.t !== "card.usage.settled" && event.t !== "lead.usage.settled") return [partial.has(index) ? { ...event, complete: false } : event];
    const e = event as T & UsageSettlement;
    if (latest.get(e.usageId)?.index !== index || e.reported === false) return [];
    return [{ ...event, t: event.t.replace(".settled", ""), at: e.occurredAt, complete: e.complete && !conflicts.has(e.usageId) }];
  });
}

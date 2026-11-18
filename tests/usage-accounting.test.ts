import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { legacyReferences, projectUsage } from "../src/core/usageAccounting.ts";

const cases = JSON.parse(readFileSync(new URL("./fixtures/usage-settlements.json", import.meta.url), "utf8"));
for (const fixture of cases) test(`usage settlements: ${fixture.name}`, () => {
  const projected = projectUsage(fixture.events) as Record<string, unknown>[];
  assert.deepEqual(projected.filter((e) => e.t === "card.usage").map(({ cardId, input, at, complete }) => ({ cardId, input, at, complete })), fixture.expected);
  const refs = legacyReferences(fixture.events);
  const stable = fixture.events.map((e: Record<string, unknown>) => Array.isArray(e.replaces) ? { ...e, replaces: e.replaces.map((i: number) => refs[i]) } : e).filter((e: { t: string }) => e.t !== "future.event");
  const stableProjected = projectUsage(stable) as Record<string, unknown>[];
  assert.deepEqual(stableProjected.filter((e) => e.t === "card.usage").map(({ cardId, input, at, complete }) => ({ cardId, input, at, complete })), fixture.expected);
});

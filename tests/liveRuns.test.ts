import assert from "node:assert/strict";
import { test } from "node:test";

import { newestFirst, parseEventLines } from "../app/dev/liveRuns.ts";
import type { OEvent } from "../src/core/events.ts";

const start = (runId: string, at: string): OEvent => ({ t: "run.start", at, runId, title: runId, project: "C:/work/demo", cards: [] });

test("a half-written last line of a live run is skipped, not fatal", () => {
  const whole = JSON.stringify(start("r1", "2026-11-01T10:00:00.000Z"));
  const message = JSON.stringify({ t: "msg", at: "2026-11-01T10:00:01.000Z", from: "lead", to: "all", text: "시작" });
  const events = parseEventLines(`${whole}\n\n${message}\r\n{"t":"card.tool","at":"2026-11-01T1`);
  assert.deepEqual(
    events.map((e) => e.t),
    ["run.start", "msg"],
  );
  assert.deepEqual(parseEventLines(""), []);
});

test("runs come newest first by their start, and runs without a start are left out", () => {
  const runs = newestFirst([
    { runId: "20261101-090000-aaaa", events: [start("20261101-090000-aaaa", "2026-11-01T09:00:00.000Z")] },
    { runId: "20261101-120000-cccc", events: [] },
    { runId: "20261101-110000-bbbb", events: [start("20261101-110000-bbbb", "2026-11-01T11:00:00.000Z")] },
  ]);
  assert.deepEqual(
    runs.map((r) => r.runId),
    ["20261101-110000-bbbb", "20261101-090000-aaaa"],
  );
});

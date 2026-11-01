import assert from "node:assert/strict";
import { test } from "node:test";

import { parseAgyUsage, parseCodexRateLimits } from "../src/core/usage.ts";

// Trimmed from real answers on 2026-11-02 (codex-cli 0.166.0, agy 1.4.8); account ids removed.
test("codex app-server rate limits become windows", () => {
  const result = {
    ordinaryUsageAllowed: true,
    rateLimits: { limitId: "codex", primary: { usedPercent: 57, windowDurationMins: 10080, resetsAt: 1793889279 }, secondary: null, planType: "prolite" },
  };
  assert.deepEqual(parseCodexRateLimits(result), [{ provider: "codex", window: "seven_day", used: 0.57, resetsAt: new Date(1793889279 * 1000).toISOString() }]);
  assert.deepEqual(parseCodexRateLimits({}), []);
});

test("agy /usage groups become windows per model group", () => {
  const stdout = JSON.stringify({
    status: "SUCCESS",
    num_turns: 0,
    command: {
      name: "usage",
      data: {
        groups: [
          {
            name: "Gemini Models",
            buckets: [
              { id: "gemini-weekly", window: "weekly", remaining_fraction: 0.9982682466506958, reset_time: "2026-11-08T09:17:28Z" },
              { id: "gemini-5h", window: "5h", remaining_fraction: 1, reset_time: "2026-11-01T19:17:28Z" },
            ],
          },
          { name: "Claude and GPT models", buckets: [{ id: "3p-weekly", window: "weekly", remaining_fraction: 0.49, reset_time: "2026-11-04T04:25:49Z" }] },
        ],
      },
    },
  });
  const windows = parseAgyUsage(stdout);
  assert.equal(windows.length, 3);
  assert.deepEqual(windows[2], { provider: "agy", window: "seven_day", group: "Claude and GPT models", used: 0.51, resetsAt: "2026-11-04T04:25:49Z" });
  assert.ok(Math.abs(windows[0].used - 0.0017) < 0.0001);
  assert.deepEqual(parseAgyUsage("not json"), []);
});

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { LIMITS } from "../src/core/limits.ts";

// The screen shows the caps the Rust CLI enforces; this keeps the copy honest.
const rust = readFileSync(new URL("../crates/o-core/src/policy.rs", import.meta.url), "utf8");
const constant = (name: string) => Number(new RegExp(`${name}: ([\\d_]+),`).exec(rust)?.[1].replace(/_/g, ""));
const perProvider = (variant: string) => Number(new RegExp(`ProviderId::${variant} => (\\d+),`).exec(rust)?.[1]);

test("the screen's caps match the O CLI's", () => {
  assert.equal(LIMITS.maxActiveTotal, constant("max_active_total"));
  assert.deepEqual(LIMITS.maxActivePerProvider, { claude: perProvider("Claude"), codex: perProvider("Codex"), agy: perProvider("Agy"), opencode: perProvider("OpenCode"), devin: perProvider("Devin") });
});

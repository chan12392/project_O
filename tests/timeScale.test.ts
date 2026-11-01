import assert from "node:assert/strict";
import { test } from "node:test";

import { timeScale } from "../app/src/timeScale.ts";

const MIN = 60_000;
const close = (a: number, b: number) => assert.ok(Math.abs(a - b) < 1e-9, `${a} ≈ ${b}`);

test("hours of idle time fold into a thin gap, so short cards stay visible", () => {
  // Two 2-minute cards four hours apart.
  const { at, stretches } = timeScale([
    [0, 2 * MIN],
    [240 * MIN, 242 * MIN],
  ]);
  assert.equal(stretches.length, 2);
  close(stretches[1].left - (stretches[0].left + stretches[0].width), 3);
  close(at(0), 0);
  close(at(242 * MIN), 100);
  // Each card gets half of the room the gap leaves, not a sliver of four hours.
  close(at(2 * MIN) - at(0), 48.5);
});

test("cards less than ten minutes apart share one stretch on a true time scale", () => {
  const { at, stretches } = timeScale([
    [0, 10 * MIN],
    [15 * MIN, 20 * MIN],
  ]);
  assert.equal(stretches.length, 1);
  close(at(10 * MIN), 50);
});

test("times outside the spans stay on the axis, and one instant still has width", () => {
  const { at, stretches } = timeScale([[5 * MIN, 5 * MIN]]);
  assert.equal(stretches.length, 1);
  assert.ok(stretches[0].to > stretches[0].from);
  close(at(0), 0);
  close(at(60 * MIN), 100);
});

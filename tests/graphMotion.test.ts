import assert from "node:assert/strict";
import { test } from "node:test";

import { boundsOf, easeOut, fitDistance, fitSettled, frameWrite, pickSphere, REVEAL_SPREAD_MS, revealDelay } from "../app/src/features/graph/graphMotion.ts";

test("a move that frames stopped in still gets its final write", () => {
  // The frames a move from t=0 to t=100 gets: one early, then none until long after its end.
  let state = { dirty: true, owed: false };
  const frame = (now: number) => {
    const { write, owed } = frameWrite(now < 100, state.dirty, state.owed);
    state = { dirty: write ? false : state.dirty, owed };
    return write;
  };
  assert.equal(frame(10), true);
  assert.equal(frame(5_000), true, "the first frame past the end writes where the move ends");
  assert.equal(frame(5_016), false, "and then the buffers rest");
  // Nothing moving and nothing changed: no write. A change alone is written once.
  assert.deepEqual(frameWrite(false, false, false), { write: false, owed: false });
  assert.deepEqual(frameWrite(false, true, false), { write: true, owed: false });
});

test("the camera's fit is used up only by the layout of the view on screen", () => {
  const settled = { loading: false, busy: false, placed: "a|b|c", current: "a|b|c" };
  assert.equal(fitSettled(settled), true);
  // A part's layout answered after the whole view arrived: the fit stays owed to the next layout.
  assert.equal(fitSettled({ ...settled, placed: "a|b" }), false);
  assert.equal(fitSettled({ ...settled, loading: true }), false);
  assert.equal(fitSettled({ ...settled, busy: true }), false);
});

test("easeOut runs 0 → 1 and holds outside the move", () => {
  assert.equal(easeOut(-1), 0);
  assert.equal(easeOut(0), 0);
  assert.equal(easeOut(1), 1);
  assert.equal(easeOut(5), 1);
  assert.ok(easeOut(0.5) > 0.5, "fast out, soft landing");
});

test("the reveal starts level by level and its spread stays bounded for a deep tree", () => {
  assert.equal(revealDelay(0, 3), 0);
  assert.equal(revealDelay(1, 3), 260);
  assert.equal(revealDelay(3, 3), 780);
  // A tree deeper than the spread allows shortens each step instead of running long.
  assert.equal(revealDelay(20, 20), REVEAL_SPREAD_MS);
  assert.ok(revealDelay(10, 20) < revealDelay(11, 20));
  // No depth means no waiting, and a level past the deepest waits no longer than the deepest.
  assert.equal(revealDelay(2, 0), 0);
  assert.equal(revealDelay(9, 3), revealDelay(3, 3));
});

test("boundsOf is the sphere around the positions", () => {
  assert.deepEqual(boundsOf([], 0), { center: [0, 0, 0], radius: 0 });
  const { center, radius } = boundsOf([-10, 0, 0, 10, 0, 0, 0, 4, 0, 0, -4, 0], 4);
  assert.deepEqual(center, [0, 0, 0]);
  assert.equal(radius, 10);
  // Only the first `count` positions count.
  assert.equal(boundsOf([0, 0, 0, 100, 0, 0], 1).radius, 0);
});

test("fitDistance holds the sphere on the narrower angle", () => {
  const wide = fitDistance(100, 60, 2);
  // 100 / sin(30°) = 200, with a margin.
  assert.ok(Math.abs(wide - 230) < 1e-6);
  assert.ok(fitDistance(100, 60, 0.5) > wide, "a tall narrow canvas stands further back");
  assert.equal(fitDistance(0, 60, 1), fitDistance(60, 60, 1), "a few close nodes are framed from a sensible distance");
});

test("pickSphere returns the nearest node the ray meets", () => {
  const origin = { x: 0, y: 0, z: 100 };
  const forward = { x: 0, y: 0, z: -1 };
  const positions = [0, 0, 0, 0, 0, 50, 30, 0, 0, 0, 0, 200];
  const radii = [5, 5, 5, 5];
  // Node 1 is in front of node 0; node 2 is off the ray; node 3 is behind the camera.
  assert.equal(pickSphere(origin, forward, positions, radii, 4), 1);
  assert.equal(pickSphere(origin, forward, positions, [5, 0, 5, 5], 4), 0);
  assert.equal(pickSphere(origin, { x: 1, y: 0, z: 0 }, positions, radii, 4), -1);
  assert.equal(pickSphere(origin, forward, positions, radii, 0), -1);
});

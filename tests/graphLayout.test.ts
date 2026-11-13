import assert from "node:assert/strict";
import { test } from "node:test";

import { layout } from "../app/src/features/graph/graphLayout.ts";

const NONE = Number.NaN;
const distance = (p: Float32Array, a: number, b: number) => Math.hypot(p[a * 3] - p[b * 3], p[a * 3 + 1] - p[b * 3 + 1], p[a * 3 + 2] - p[b * 3 + 2]);

test("layout places every node at a finite position, linked nodes nearer than unlinked ones", () => {
  // A chain 0–1–2–3 and a loose node 4.
  const input = { count: 5, seed: new Float32Array(15).fill(NONE), links: Uint32Array.from([0, 1, 1, 2, 2, 3]) };
  const positions = layout(input);
  assert.equal(positions.length, 15);
  assert.ok([...positions].every(Number.isFinite));
  assert.ok(distance(positions, 0, 1) < distance(positions, 0, 3));
  // The same input gives the same picture.
  assert.deepEqual([...layout({ ...input, seed: new Float32Array(15).fill(NONE) })], [...positions]);
});

test("layout keeps a settled graph in place when it only gains a node", () => {
  const first = layout({ count: 4, seed: new Float32Array(12).fill(NONE), links: Uint32Array.from([0, 1, 0, 2, 0, 3]) });
  const seed = new Float32Array(15).fill(NONE);
  seed.set(first);
  const next = layout({ count: 5, seed, links: Uint32Array.from([0, 1, 0, 2, 0, 3, 3, 4]) });
  for (let i = 0; i < 4; i++) {
    const moved = Math.hypot(next[i * 3] - first[i * 3], next[i * 3 + 1] - first[i * 3 + 1], next[i * 3 + 2] - first[i * 3 + 2]);
    assert.ok(moved < 30, `node ${i} moved ${moved}`);
  }
  assert.ok([...next].every(Number.isFinite));
});

test("layout ignores links that point outside the nodes and answers an empty graph", () => {
  assert.equal(layout({ count: 0, seed: new Float32Array(0), links: new Uint32Array(0) }).length, 0);
  const positions = layout({ count: 2, seed: new Float32Array(6).fill(NONE), links: Uint32Array.from([0, 7, 1, 1, 0]) });
  assert.ok([...positions].every(Number.isFinite));
});

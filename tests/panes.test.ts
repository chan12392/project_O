import assert from "node:assert/strict";
import { test } from "node:test";

import { defaultPaneCount, fitPanes, showInPane } from "../src/core/panes.ts";

test("a team opens in two panes, a lone agent in one", () => {
  assert.equal(defaultPaneCount(0), 1);
  assert.equal(defaultPaneCount(1), 1);
  assert.equal(defaultPaneCount(11), 2);
});

test("panes keep the open agents and fill the rest in team order", () => {
  const ids = ["a", "b", "c", "d", "e"];
  assert.deepEqual(fitPanes([], ids, 2), ["a", "b"]);
  assert.deepEqual(fitPanes(["d"], ids, 4), ["d", "a", "b", "c"]);
  // Fewer panes keep the first open ones; agents that left the team drop out.
  assert.deepEqual(fitPanes(["c", "a", "b", "d"], ids, 2), ["c", "a"]);
  assert.deepEqual(fitPanes(["gone", "b"], ids, 2), ["b", "a"]);
  // Never more panes than agents, never one agent twice.
  assert.deepEqual(fitPanes(["a", "a"], ["a"], 4), ["a"]);
  assert.deepEqual(fitPanes([], [], 2), []);
});

test("a tab opens in the focused pane, or focuses the pane already showing it", () => {
  assert.deepEqual(showInPane(["a", "b"], 1, "c"), { open: ["a", "c"], focus: 1 });
  assert.deepEqual(showInPane(["a", "b"], 1, "a"), { open: ["a", "b"], focus: 0 });
  assert.deepEqual(showInPane(["a", "b"], 5, "c"), { open: ["a", "c"], focus: 1 });
  assert.deepEqual(showInPane([], 0, "a"), { open: ["a"], focus: 0 });
});

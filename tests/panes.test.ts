import assert from "node:assert/strict";
import { test } from "node:test";

import { EMPTY_COMPOSER, defaultPaneCount, fitPaneCount, fitPanes, showInPane, withComposer } from "../src/core/panes.ts";

test("a team opens in two panes, a lone agent in one", () => {
  assert.equal(defaultPaneCount(0), 1);
  assert.equal(defaultPaneCount(1), 1);
  assert.equal(defaultPaneCount(11), 2);
});

test("a saved split larger than the team shrinks to fit, so no pane sits empty", () => {
  assert.equal(fitPaneCount(2, 1), 1);
  assert.equal(fitPaneCount(4, 1), 1);
  assert.equal(fitPaneCount(4, 2), 2);
  assert.equal(fitPaneCount(4, 3), 4);
  assert.equal(fitPaneCount(2, 5), 2);
  assert.equal(fitPaneCount(1, 5), 1);
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

test("an unsent follow-up stays with its card while another tab or fewer panes hide it (#55)", () => {
  const ids = ["a", "b", "c"];
  let composers = withComposer({}, "a", { draft: "A에게 보낼 지시" });
  // A -> C -> A in the focused pane, then four panes down to one and back.
  let open = showInPane(fitPanes([], ids, 2), 0, "c").open;
  assert.deepEqual(open, ["c", "b"]);
  open = fitPanes(open, ids, 1);
  assert.ok(!open.includes("a"));
  open = showInPane(open, 0, "a").open;
  assert.deepEqual(open, ["a"]);
  assert.equal(composers.a.draft, "A에게 보낼 지시");
  // Other cards keep their own, empty composer; a send in flight clears only its card.
  assert.equal(composers.b, undefined);
  composers = withComposer(composers, "b", { draft: "B", sending: true });
  composers = withComposer(withComposer(composers, "b", { draft: "" }), "b", { sending: false });
  assert.deepEqual(composers.b, EMPTY_COMPOSER);
  assert.deepEqual(composers.a, { draft: "A에게 보낼 지시", sending: false });
});

test("composer drafts survive card ids that collide with Object.prototype (#76)", () => {
  let composers = withComposer({}, "constructor", { draft: "x" });
  assert.ok(Object.hasOwn(composers, "constructor"));
  assert.equal(composers["constructor"].draft, "x");
  // Other prototype-named ids start from an empty composer, not a prototype member.
  for (const id of ["__proto__", "toString"]) {
    composers = withComposer(composers, id, { draft: id });
    assert.equal(composers[id].draft, id);
  }
  assert.deepEqual(withComposer({}, "toString", {}), { toString: EMPTY_COMPOSER });
  assert.equal(composers["toString"].draft, "toString");
  assert.equal(composers["constructor"].draft, "x");
});

import assert from "node:assert/strict";
import { test } from "node:test";

import { popoutUrl } from "../app/src/shell/popoutUrl.ts";

test("the pop-out keeps the runs source the main window shows", () => {
  const target = { view: "work", project: "C:/work/app", run: "20261101-100916-402x", card: "a" };
  // A browser window that fell back to the demo (no ?source=) opens a demo pop-out on the same card.
  const demo = popoutUrl("http://127.0.0.1:1430/?run=x&t=0.5", target, "demo");
  assert.equal(demo.searchParams.get("source"), "demo");
  assert.equal(demo.searchParams.get("popout"), "panel");
  assert.deepEqual(
    ["view", "project", "run", "card"].map((k) => demo.searchParams.get(k)),
    ["work", "C:/work/app", "20261101-100916-402x", "a"],
  );
  assert.equal(demo.searchParams.get("t"), null, "the main window's other parameters stay behind");

  assert.equal(popoutUrl("http://127.0.0.1:1430/?source=demo", target, "live").searchParams.get("source"), "live", "the effective source, not the address's");
  assert.equal(popoutUrl("http://127.0.0.1:1430/", { view: "office" }).searchParams.has("source"), false, "undecided: left to the pop-out");
});

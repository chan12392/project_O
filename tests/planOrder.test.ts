import assert from "node:assert/strict";
import { test } from "node:test";

import { planOrder, planOrderText } from "../src/core/lead.ts";

const write = (key: string) => [{ kind: "project-path" as const, key, mode: "write" as const }];

test("cards without dependencies start together", () => {
  const cards = [{ id: "core", resources: write("src/core") }, { id: "gui", resources: write("src/gui") }];
  assert.deepEqual(planOrder(cards), { stages: [["core", "gui"]], alone: [] });
  assert.equal(planOrderText(cards), "2개 작업 모두 동시에 시작");
});

test("a depends_on chain shows as stages in order", () => {
  const cards = [
    { id: "core", resources: write("core") },
    { id: "gui", depends_on: ["core"], resources: write("gui") },
    { id: "docs", resources: write("docs") },
    { id: "review", depends_on: ["gui", "docs"], resources: write("review") },
  ];
  assert.deepEqual(planOrder(cards).stages, [["core", "docs"], ["gui"], ["review"]]);
  assert.equal(planOrderText(cards), "3단계로 차례 실행: core, docs → gui → review");
});

test("undeclared resources and broken dependencies are named, not hidden", () => {
  const cards = [{ id: "a" }, { id: "b", depends_on: ["missing"], resources: write("b") }, { id: "c", depends_on: ["c"], resources: write("c") }];
  assert.deepEqual(planOrder(cards), { stages: [["a"], ["b", "c"]], alone: ["a"] });
  assert.match(planOrderText(cards), /자원 미선언\(a\)은 혼자 실행$/);
});

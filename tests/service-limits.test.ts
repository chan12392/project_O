import assert from "node:assert/strict";
import { test } from "node:test";

import { serviceLimits } from "../app/src/shell/serviceLimits.ts";
import type { CardView } from "../src/core/snapshot.ts";

test("status reports exhausted budgets and does not treat unknown budgets as clear", () => {
  assert.match(serviceLimits([], undefined).text, /예산 확인 전/);
  assert.match(serviceLimits([], [{ provider: "codex", state: "unknown", message: "확인 불가" }]).text, /예산 확인 전/);
  const exhausted = serviceLimits([], [{ provider: "codex", state: "over", message: "내일 다시 열림" }]);
  assert.match(exhausted.text, /오늘 예산 소진: Codex/);
  assert.equal(exhausted.title, "내일 다시 열림");
  // Nothing to report: the usage chips carry the rest, so the line is left out.
  assert.equal(serviceLimits([], [{ provider: "codex", state: "ok", message: "정상" }]).text, "");
});

test("a card whose CLI hit its subscription limit is named, apart from the budget", () => {
  const card = { id: "a", provider: "claude", label: "a", state: "failed", activity: "", log: [{ at: "2026-11-12T00:00:00.000Z", kind: "limit", text: "한도" }], usage: { input: 0, cached: 0, output: 0 } } satisfies CardView;
  const hit = serviceLimits([card], [{ provider: "claude", state: "ok", message: "정상" }]);
  assert.equal(hit.text, "구독 한도에 걸린 카드: Claude");
  assert.equal(hit.title, "카드 실행 중 CLI가 구독 사용 한도에 걸림");
});

test("with the daily budget off nothing reads as exhausted or waiting", () => {
  const off = serviceLimits([], [
    { provider: "codex", state: "off", message: "codex 하루 사용 예산 꺼짐" },
    { provider: "claude", state: "off", message: "claude 하루 사용 예산 꺼짐" },
  ]);
  assert.equal(off.text, "하루 예산 꺼짐");
  assert.doesNotMatch(off.text, /소진|확인 전/);
});

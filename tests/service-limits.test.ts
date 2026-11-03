import assert from "node:assert/strict";
import { test } from "node:test";

import { serviceLimits } from "../app/src/shell/serviceLimits.ts";

test("status reports exhausted budgets and does not treat unknown budgets as clear", () => {
  assert.match(serviceLimits([], undefined).text, /예산 확인 전/);
  assert.match(serviceLimits([], [{ provider: "codex", state: "unknown", message: "확인 불가" }]).text, /예산 확인 전/);
  const exhausted = serviceLimits([], [{ provider: "codex", state: "over", message: "내일 다시 열림" }]);
  assert.match(exhausted.text, /오늘 예산 소진: Codex/);
  assert.equal(exhausted.title, "내일 다시 열림");
  assert.equal(serviceLimits([], [{ provider: "codex", state: "ok", message: "정상" }]).text, "한도 걸린 서비스 없음");
});

test("with the daily budget off nothing reads as exhausted or waiting", () => {
  const off = serviceLimits([], [
    { provider: "codex", state: "off", message: "codex 하루 사용 예산 꺼짐" },
    { provider: "claude", state: "off", message: "claude 하루 사용 예산 꺼짐" },
  ]);
  assert.equal(off.text, "한도 걸린 서비스 없음 · 하루 예산 꺼짐");
  assert.doesNotMatch(off.text, /소진|확인 전/);
});

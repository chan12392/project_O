import assert from "node:assert/strict";
import { test } from "node:test";

import { latestStreamQuotas, usageChips } from "../app/src/features/usage/statusUsage.ts";
import type { ProviderId } from "../src/core/plan.ts";
import type { CardView, RunView } from "../src/core/snapshot.ts";
import type { UsageWindow } from "../src/core/usage.ts";

// Local parts, so the clock times read the same in any time zone.
const NOW = new Date(2026, 10, 12, 12, 0);
const AT_1440 = new Date(2026, 10, 12, 14, 40).toISOString();
const AT_1610 = new Date(2026, 10, 12, 16, 10).toISOString();
const SUNDAY_0900 = new Date(2026, 10, 15, 9, 0).toISOString();
const EARLIER = new Date(2026, 10, 12, 11, 0).toISOString();

function five(provider: ProviderId, used: number, resetsAt = AT_1440, group?: string): UsageWindow {
  return { provider, window: "five_hour", used, resetsAt, ...(group ? { group } : {}) };
}

function week(provider: ProviderId, used: number, resetsAt = SUNDAY_0900): UsageWindow {
  return { provider, window: "seven_day", used, resetsAt };
}

function card(id: string, provider: ProviderId, state: CardView["state"] = "done"): CardView {
  return { id, provider, label: `${id} 카드`, state, activity: "", log: [], usage: { input: 0, cached: 0, output: 0 } };
}

function run(runId: string, cards: CardView[], quotas: RunView["quotas"] = []): RunView {
  return { runId, title: runId, project: "C:/p/alpha", startedAt: "2026-11-12T00:00:00.000Z", status: "running", cards, messages: [], quotas };
}

function quota(w: UsageWindow, at: string): RunView["quotas"][number] {
  return { provider: w.provider, window: w.window, used: w.used, resetsAt: w.resetsAt, at };
}

function only(runs: RunView[], windows: UsageWindow[], now = NOW) {
  const chips = usageChips(runs, windows, now);
  assert.equal(chips.length, 1);
  return chips[0];
}

test("a fresh 5-hour window shows its share and reset time; the weekly one stays in the tooltip and label", () => {
  const chip = only([], [five("claude", 0.32), week("claude", 0.57)]);
  assert.deepEqual(chip, {
    provider: "claude",
    name: "Claude",
    value: "5h 32%",
    rest: " · 14:40 리셋",
    tone: "normal",
    title: "Claude\n5시간 32% · 14:40 리셋\n주간 57% · 11월 15일 09:00 리셋",
    label: "Claude 5시간 32%, 14:40 리셋. 주간 57%, 11월 15일 09:00 리셋",
  });
  assert.doesNotMatch(chip.value + chip.rest, /57|11월/);
});

test("windows from o usage and quotas in the run streams both make chips", () => {
  const runs = [run("r-1", [card("a", "claude")], [quota(five("claude", 0.32), "2026-11-12T01:00:00.000Z")])];
  const chips = usageChips(runs, [five("codex", 0.41, AT_1610)], NOW);
  assert.deepEqual(
    chips.map((c) => [c.provider, c.value, c.rest]),
    [
      ["claude", "5h 32%", " · 14:40 리셋"],
      ["codex", "5h 41%", " · 16:10 리셋"],
    ],
  );
});

test("stream quotas keep the newest report per provider and window across runs", () => {
  const runs = [
    run("r-1", [], [quota(five("claude", 0.1), "2026-11-12T01:00:00.000Z"), quota(week("claude", 0.5), "2026-11-12T03:00:00.000Z")]),
    run("r-2", [], [quota(five("claude", 0.32), "2026-11-12T02:00:00.000Z"), quota(week("claude", 0.4), "2026-11-12T01:30:00.000Z")]),
    run("r-3", [], [quota(five("codex", 0.2, AT_1610), "2026-11-12T00:30:00.000Z")]),
  ];
  assert.deepEqual(latestStreamQuotas(runs), [five("claude", 0.32), week("claude", 0.5), five("codex", 0.2, AT_1610)]);
  assert.deepEqual(latestStreamQuotas([]), []);
  assert.deepEqual(
    usageChips(runs, [], NOW).map((c) => [c.provider, c.value]),
    [
      ["claude", "5h 32%"],
      ["codex", "5h 20%"],
    ],
  );
});

test("model groups: the chip shows the fullest group and the tooltip lists every group", () => {
  const chip = only([], [five("agy", 0.2, AT_1440, "pro"), five("agy", 0.65, AT_1610, "flash"), five("agy", 0.05, AT_1440, "lite")]);
  assert.equal(chip.name, "Agy");
  assert.equal(chip.value, "5h 65%");
  assert.equal(chip.rest, " · 16:10 리셋");
  assert.equal(chip.tone, "normal");
  assert.equal(chip.title, "Antigravity\n5시간 pro 20% · 14:40 리셋\n5시간 flash 65% · 16:10 리셋\n5시간 lite 5% · 14:40 리셋\n주간: CLI가 알려 주지 않음");
  assert.equal(chip.label, "Antigravity 5시간 flash 65%, 16:10 리셋. 주간 알 수 없음");
});

test("tone warns from 80% used and turns to danger at the limit", () => {
  assert.equal(only([], [five("claude", 0.79)]).tone, "normal");
  assert.equal(only([], [five("claude", 0.799)]).tone, "normal", "rounding the label does not exhaust a window");
  assert.equal(only([], [five("claude", 0.999)]).tone, "warn");
  const warn = only([], [five("claude", 0.8)]);
  assert.equal(warn.value, "5h 80%");
  assert.equal(warn.tone, "warn");
  assert.match(warn.title, /\n80% 이상 사용$/);
  assert.match(warn.label, /\. 80% 이상 사용$/);
  const out = only([], [five("claude", 1)]);
  assert.equal(out.value, "5h 100%");
  assert.equal(out.tone, "danger");
  assert.match(out.title, /\n한도에 걸림$/);
  assert.match(out.label, /\. 한도에 걸림$/);
});

test("a weekly window at 80% or more warns while the chip keeps the low 5-hour figure", () => {
  const chip = only([], [five("claude", 0.1), week("claude", 0.85)]);
  assert.equal(chip.value, "5h 10%");
  assert.equal(chip.rest, " · 14:40 리셋");
  assert.equal(chip.tone, "warn");
  assert.equal(chip.title, "Claude\n5시간 10% · 14:40 리셋\n주간 85% · 11월 15일 09:00 리셋\n80% 이상 사용");
});

test("a weekly window at its limit takes the chip's place", () => {
  assert.equal(only([], [five("claude", 0.1), week("claude", 0.999)]).value, "5h 10%");
  const chip = only([], [five("claude", 0.1), week("claude", 1)]);
  assert.equal(chip.value, "주간 100%");
  assert.equal(chip.rest, " · 11월 15일 09:00 리셋");
  assert.equal(chip.tone, "danger");
  assert.equal(chip.title, "Claude\n5시간 10% · 14:40 리셋\n주간 100% · 11월 15일 09:00 리셋\n한도에 걸림");
  assert.equal(chip.label, "Claude 5시간 10%, 14:40 리셋. 주간 100%, 11월 15일 09:00 리셋. 한도에 걸림");
});

test("a window at or past its reset reads as reset and asks for a new reading", () => {
  for (const resetsAt of [EARLIER, NOW.toISOString()]) {
    const chip = only([], [five("claude", 0.95, resetsAt)]);
    assert.equal(chip.value, "5h 리셋됨");
    assert.equal(chip.rest, ", 새로 읽기 필요");
    assert.equal(chip.tone, "muted");
    assert.equal(chip.title, "Claude\n5시간 리셋됨, 새로 읽기 필요\n주간: CLI가 알려 주지 않음");
    assert.equal(chip.label, "Claude 5시간 리셋됨, 새로 읽기 필요. 주간 알 수 없음");
  }
  const oneMinuteLeft = only([], [five("claude", 0.95, new Date(2026, 10, 12, 12, 1).toISOString())]);
  assert.equal(oneMinuteLeft.value, "5h 95%");
  assert.equal(oneMinuteLeft.tone, "warn");
});

test("a group past its reset gives way to one still running", () => {
  const chip = only([], [five("agy", 0.9, EARLIER, "pro"), five("agy", 0.3, AT_1440, "flash")]);
  assert.equal(chip.value, "5h 30%");
  assert.equal(chip.rest, " · 14:40 리셋");
  assert.equal(chip.tone, "normal");
  assert.equal(chip.title, "Antigravity\n5시간 pro 리셋됨, 새로 읽기 필요\n5시간 flash 30% · 14:40 리셋\n주간: CLI가 알려 주지 않음");

  const allPast = only([], [five("agy", 0.9, EARLIER, "pro"), five("agy", 0.3, EARLIER, "flash")]);
  assert.equal(allPast.value, "5h 리셋됨");
  assert.equal(allPast.tone, "muted");
});

test("with only a weekly window known the chip shows it with the reset date", () => {
  const chip = only([], [week("codex", 0.57)]);
  assert.equal(chip.name, "Codex");
  assert.equal(chip.value, "주간 57%");
  assert.equal(chip.rest, " · 11월 15일 09:00 리셋");
  assert.equal(chip.tone, "normal");
  assert.equal(chip.title, "Codex\n5시간: CLI가 알려 주지 않음\n주간 57% · 11월 15일 09:00 리셋");
  assert.equal(chip.label, "Codex 5시간 알 수 없음. 주간 57%, 11월 15일 09:00 리셋");
});

test("a metered agent with cards but no window shows a muted dash; one with neither gets no chip", () => {
  const chip = only([run("r-1", [card("a", "codex", "running")])], []);
  assert.equal(chip.provider, "codex");
  assert.equal(chip.value, "5h –");
  assert.equal(chip.rest, "");
  assert.equal(chip.tone, "muted");
  assert.equal(chip.title, "Codex\n5시간: CLI가 알려 주지 않음\n주간: CLI가 알려 주지 않음");
  assert.equal(chip.label, "Codex 5시간 알 수 없음. 주간 알 수 없음");
  assert.deepEqual(usageChips([], [], NOW), []);
  assert.deepEqual(usageChips([run("r-2", [])], [], NOW), []);
});

test("a bucket under the CLI's own id is no chip figure but the tooltip tells it", () => {
  const chip = only([], [{ provider: "agy", window: "gemini-weekly", group: "pro", used: 0.4, resetsAt: SUNDAY_0900 }]);
  assert.equal(chip.value, "5h –");
  assert.equal(chip.tone, "muted");
  assert.equal(chip.title, "Antigravity\n5시간: CLI가 알려 주지 않음\n주간: CLI가 알려 주지 않음\ngemini-weekly pro 40% · 11월 15일 09:00 리셋");
});

test("opencode and devin count their running cards and never show a percentage", () => {
  assert.deepEqual(usageChips([], [five("opencode", 0.5), week("devin", 0.9)], NOW), []);

  const runs = [
    run("r-1", [card("o1", "opencode", "running"), card("o2", "opencode", "done"), card("d1", "devin", "queued")]),
    run("r-2", [card("o3", "opencode", "running"), card("d2", "devin", "failed")]),
  ];
  const chips = usageChips(runs, [five("opencode", 0.5), week("devin", 0.9)], NOW);
  assert.deepEqual(
    chips.map((c) => [c.provider, c.name, c.value, c.rest, c.tone]),
    [
      ["opencode", "OpenCode", "실행 중 2", "", "normal"],
      ["devin", "Devin", "실행 중 0", "", "muted"],
    ],
  );
  for (const c of chips) assert.doesNotMatch(c.value + c.rest + c.title + c.label, /%/);
});

test("metered agents come first as claude, codex, agy; the card counters follow", () => {
  const runs = [run("r-1", [card("d", "devin", "running"), card("o", "opencode", "running"), card("c", "claude")])];
  const windows = [five("agy", 0.1), five("codex", 0.2)];
  assert.deepEqual(
    usageChips(runs, windows, NOW).map((c) => c.provider),
    ["claude", "codex", "agy", "opencode", "devin"],
  );
});

import { LIMITS } from "../../../../src/core/limits.ts";
import type { ProviderId } from "../../../../src/core/plan.ts";
import type { RunView } from "../../../../src/core/snapshot.ts";
import type { UsageWindow } from "../../../../src/core/usage.ts";
import { PROVIDER_NAME, PROVIDER_SHORT, timeOf } from "../../lib/format.ts";

/** The agents whose CLI reports subscription windows, in display order. */
export const METERED: ProviderId[] = ["claude", "codex", "agy"];
/** Their CLIs report no window: the status bar counts their running cards instead. */
const UNMETERED: ProviderId[] = ["opencode", "devin"];

/** Claude reports its windows in each run's stream; keep the newest per window. */
export function latestStreamQuotas(runs: readonly RunView[]): UsageWindow[] {
  const latest = new Map<string, RunView["quotas"][number]>();
  for (const q of runs.flatMap((r) => r.quotas)) {
    const key = `${q.provider}/${q.window}`;
    const seen = latest.get(key);
    if (!seen || seen.at < q.at) latest.set(key, q);
  }
  return [...latest.values()].map(({ provider, window, used, resetsAt }) => ({ provider, window, used, resetsAt }));
}

/** The fullest of the windows: where a CLI splits one by model group (agy), the group that runs out first. */
export function fullest(windows: readonly UsageWindow[]): UsageWindow | undefined {
  return windows.reduce<UsageWindow | undefined>((w, x) => (!w || x.used > w.used ? x : w), undefined);
}

/** One agent in the status bar: its 5-hour window, or its running cards when it has no usage to read. */
export interface UsageChip {
  provider: ProviderId;
  name: string;
  /** The figure: "5h 32%", "주간 57%", "5h 리셋됨", "실행 중 2". */
  value: string;
  /** What a narrow bar drops first: " · 14:40 리셋", ", 새로 읽기 필요". */
  rest: string;
  /** warn from 80% used, danger at the limit, muted when there is no figure to trust. */
  tone: "normal" | "warn" | "danger" | "muted";
  /** Tooltip: every window and model group, one per line. */
  title: string;
  /** The same in one sentence, for a screen reader. */
  label: string;
}

const dayClock = new Intl.DateTimeFormat("ko-KR", { month: "long", day: "numeric", hour: "2-digit", minute: "2-digit", hourCycle: "h23" });
const WARN_PERCENT = 80;
const STALE = "리셋됨, 새로 읽기 필요";

/** The two windows every metered CLI shares; the weekly one resets on another day, so it carries the date. */
const KINDS = [
  { id: "five_hour", short: "5h", name: "5시간", when: timeOf },
  { id: "seven_day", short: "주간", name: "주간", when: (iso: string) => dayClock.format(new Date(iso)) },
];
type Kind = (typeof KINDS)[number];

const percent = (w: UsageWindow) => Math.round(w.used * 100);
/** Past its reset the reading says nothing about the window that replaced it. */
const past = (w: UsageWindow, now: Date) => Date.parse(w.resetsAt) <= now.getTime();
const resetOf = (kind: Kind, w: UsageWindow) => (Number.isNaN(Date.parse(w.resetsAt)) ? "" : `${kind.when(w.resetsAt)} 리셋`);

/** A window in words: "5시간 32% · 14:40 리셋", with its model group when the CLI names one. */
function line(kind: Kind, w: UsageWindow, now: Date, separator: string): string {
  const head = `${kind.name} ${w.group ? `${w.group} ` : ""}`;
  if (past(w, now)) return `${head}${STALE}`;
  const reset = resetOf(kind, w);
  return `${head}${percent(w)}%${reset && separator}${reset}`;
}

/** One kind of window for one agent: all its groups, and the one the chip shows. */
function read(kind: Kind, windows: readonly UsageWindow[], now: Date) {
  const all = windows.filter((w) => w.window === kind.id);
  const running = all.filter((w) => !past(w, now));
  // Every group past its reset: any of them says the same thing.
  return { kind, all, pick: fullest(running) ?? all[0], stale: running.length === 0 };
}

/**
 * The status bar's usage chips, from the same readings as the usage panel: `o usage`'s windows and
 * the newest quota in the run streams. Agents show when a listed run used them or a window names them.
 * A chip shows the 5-hour window; the weekly one takes its place when it is the only one known or
 * has hit its limit (that is the reset to wait for).
 */
export function usageChips(runs: readonly RunView[], windows: readonly UsageWindow[], now: Date): UsageChip[] {
  const all = [...windows, ...latestStreamQuotas(runs)];
  const cards = runs.flatMap((r) => r.cards);
  const chips: UsageChip[] = [];

  for (const provider of METERED) {
    const mine = all.filter((w) => w.provider === provider);
    if (mine.length === 0 && !cards.some((c) => c.provider === provider)) continue;
    const [five, week] = KINDS.map((kind) => read(kind, mine, now));
    const full = PROVIDER_NAME[provider];
    const base = { provider, name: PROVIDER_SHORT[provider] };
    const live = [five, week].flatMap((r) => (r.pick && !r.stale ? [r.pick.used] : []));
    const worst = Math.max(0, ...live);
    const weekOut = week.pick && !week.stale && week.pick.used >= 1;
    const shown = weekOut || !five.pick ? week : five;
    const pick = shown.pick;
    const tone = worst >= 1 ? "danger" : worst >= WARN_PERCENT / 100 ? "warn" : shown.stale ? "muted" : "normal";
    const note = tone === "danger" ? "한도에 걸림" : tone === "warn" ? `${WARN_PERCENT}% 이상 사용` : "";
    const reset = pick ? resetOf(shown.kind, pick) : "";
    const detail = [five, week].flatMap((r) => (r.all.length > 0 ? r.all.map((w) => line(r.kind, w, now, " · ")) : [`${r.kind.name}: CLI가 알려 주지 않음`]));
    // A bucket the CLI names its own way (agy) is no chip figure: the tooltip tells it as it came.
    const others = mine.filter((w) => !KINDS.some((k) => k.id === w.window)).map((w) => line({ ...week.kind, name: w.window }, w, now, " · "));
    const spoken = [five, week].map((r) => (r.pick ? line(r.kind, r.pick, now, ", ") : `${r.kind.name} 알 수 없음`));
    chips.push({
      ...base,
      value: !pick ? `${five.kind.short} –` : shown.stale ? `${shown.kind.short} 리셋됨` : `${shown.kind.short} ${percent(pick)}%`,
      rest: !pick ? "" : shown.stale ? ", 새로 읽기 필요" : reset && ` · ${reset}`,
      tone,
      title: [full, ...detail, ...others, note].filter(Boolean).join("\n"),
      label: [`${full} ${spoken.join(". ")}`, note].filter(Boolean).join(". "),
    });
  }

  for (const provider of UNMETERED) {
    const mine = cards.filter((c) => c.provider === provider);
    if (mine.length === 0) continue;
    const running = mine.filter((c) => c.state === "running").length;
    const full = PROVIDER_NAME[provider];
    chips.push({
      provider,
      name: PROVIDER_SHORT[provider],
      value: `실행 중 ${running}`,
      rest: "",
      tone: running > 0 ? "normal" : "muted",
      title: `${full}\n사용량을 조회할 수 없는 서비스\n실행 중 카드 ${running}장 / 동시 ${LIMITS.maxActivePerProvider[provider]}장`,
      label: `${full} 사용량 조회 불가, 실행 중 카드 ${running}장`,
    });
  }
  return chips;
}

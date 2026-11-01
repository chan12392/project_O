import { tokensOf } from "../../../src/core/attention.ts";
import type { ProviderId } from "../../../src/core/plan.ts";
import type { RunView } from "../../../src/core/snapshot.ts";
import type { UsageWindow } from "../../../src/core/usage.ts";
import { PROVIDER_NAME, compact, timeOf } from "../format.ts";
import { Avatar } from "./Avatar.tsx";

const ORDER: ProviderId[] = ["claude", "codex", "agy"];
const RINGS: { window: string; short: string; name: string }[] = [
  { window: "five_hour", short: "5h", name: "5시간" },
  { window: "seven_day", short: "주", name: "주간" },
];

interface Props {
  runs: RunView[];
  /** Windows read by `o usage` (Codex app-server, agy /usage); Claude's come from its run streams. */
  windows: UsageWindow[];
}

/**
 * Every agent in use on one row: tokens its CLI reported, and a ring per
 * subscription window. Where a CLI splits a window by model group (agy),
 * the ring shows the fullest group and the tooltip lists them all.
 */
export function UsagePanel({ runs, windows }: Props) {
  const all = [...windows, ...latestStreamQuotas(runs)];
  const providers = ORDER.filter((p) => runs.some((r) => r.cards.some((c) => c.provider === p)) || all.some((w) => w.provider === p));
  if (providers.length === 0) return null;

  return (
    <section aria-labelledby="usage-title" className="px-5">
      <h3 id="usage-title" className="mb-2 flex items-baseline gap-2 text-2xs font-semibold tracking-wider text-muted-foreground uppercase">
        에이전트 사용량
        <span className="truncate font-normal tracking-normal normal-case">토큰은 작업 {runs.length}개 합계 · 고리는 구독 한도</span>
      </h3>
      <ul className="grid auto-cols-fr grid-flow-col divide-x rounded-xl border bg-card text-card-foreground">
        {providers.map((provider) => {
          const cards = runs.flatMap((r) => r.cards.filter((c) => c.provider === provider));
          const tokens = cards.reduce((sum, c) => sum + tokensOf(c), 0);
          const mine = all.filter((w) => w.provider === provider);
          return (
            <li key={provider} className="flex min-w-0 items-center gap-1.5 px-2 py-2">
              <Avatar speaker={provider} size="sm" />
              <span className="min-w-0 flex-1">
                <span className="block truncate text-xs font-medium">{PROVIDER_NAME[provider]}</span>
                <span className="block truncate text-3xs text-muted-foreground tabular-nums">{tokens > 0 ? compact.format(tokens) : `카드 ${cards.length}장`}</span>
              </span>
              <span className="flex shrink-0 gap-0.5">
                {RINGS.map((ring) => (
                  <Ring key={ring.window} short={ring.short} name={`${PROVIDER_NAME[provider]} ${ring.name}`} windows={mine.filter((w) => w.window === ring.window)} />
                ))}
              </span>
            </li>
          );
        })}
      </ul>
    </section>
  );
}

/** Pattern from beakho-design lucian-099 (activity ring): one arc per window, fuller is warmer. */
function Ring({ short, name, windows }: { short: string; name: string; windows: UsageWindow[] }) {
  const worst = windows.reduce<UsageWindow | undefined>((w, x) => (!w || x.used > w.used ? x : w), undefined);
  const size = 26;
  const r = 10.5;
  const length = 2 * Math.PI * r;
  if (!worst) {
    return (
      <span title={`${name}: CLI가 알려 주지 않음`} className="relative grid size-[26px] place-items-center text-3xs text-muted-foreground">
        <svg viewBox={`0 0 ${size} ${size}`} className="absolute inset-0" aria-hidden="true">
          <circle cx={13} cy={13} r={r} fill="none" strokeWidth={2.5} strokeDasharray="2 3" className="stroke-muted" />
        </svg>
        <span className="sr-only">{name} 알 수 없음</span>
        <span aria-hidden="true">{short}</span>
      </span>
    );
  }
  const percent = Math.round(worst.used * 100);
  const tone = worst.used >= 0.9 ? "stroke-red-500" : worst.used >= 0.7 ? "stroke-amber-500" : "stroke-emerald-500";
  const detail = windows
    .map((w) => `${w.group ? `${w.group} ` : ""}${Math.round(w.used * 100)}% · ${timeOf(w.resetsAt)} 초기화`)
    .join("\n");
  return (
    <span
      role="meter"
      aria-label={`${name} 사용`}
      aria-valuenow={percent}
      aria-valuemin={0}
      aria-valuemax={100}
      title={`${name}\n${detail}`}
      className="relative grid size-[26px] place-items-center text-3xs tabular-nums"
    >
      <svg viewBox={`0 0 ${size} ${size}`} className="absolute inset-0 -rotate-90" aria-hidden="true">
        <circle cx={13} cy={13} r={r} fill="none" strokeWidth={2.5} className="stroke-muted" />
        <circle cx={13} cy={13} r={r} fill="none" strokeWidth={2.5} strokeLinecap="round" strokeDasharray={`${(length * Math.min(1, worst.used)).toFixed(2)} ${length.toFixed(2)}`} className={tone} />
      </svg>
      <span aria-hidden="true">{percent}</span>
    </span>
  );
}

/** Claude reports its windows in each run's stream; keep the newest per window. */
function latestStreamQuotas(runs: RunView[]): UsageWindow[] {
  const latest = new Map<string, RunView["quotas"][number]>();
  for (const q of runs.flatMap((r) => r.quotas)) {
    const key = `${q.provider}/${q.window}`;
    const seen = latest.get(key);
    if (!seen || seen.at < q.at) latest.set(key, q);
  }
  return [...latest.values()].map(({ provider, window, used, resetsAt }) => ({ provider, window, used, resetsAt }));
}

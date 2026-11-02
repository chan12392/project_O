import type { ReactNode } from "react";

import { tokensOf } from "../../../../src/core/attention.ts";
import type { ProviderId } from "../../../../src/core/plan.ts";
import type { RunView } from "../../../../src/core/snapshot.ts";
import type { UsageWindow } from "../../../../src/core/usage.ts";
import { useBudget } from "../../data/useBudget.ts";
import type { ProviderBudget } from "../../platform/backend/index.ts";
import { PROVIDER_NAME, compact, timeOf } from "../../lib/format.ts";
import { Avatar } from "../office/Avatar.tsx";

const ORDER: ProviderId[] = ["claude", "codex", "agy"];
const RINGS: { window: string; name: string }[] = [
  { window: "five_hour", name: "5시간" },
  { window: "seven_day", name: "주간" },
];

interface Props {
  runs: RunView[];
  /** Windows read by `o usage` (Codex app-server, agy /usage); Claude's come from its run streams. */
  windows: UsageWindow[];
}

/**
 * Every agent in use on one row: tokens its CLI reported, a ring per
 * subscription window, and today's ring: the share of the day's budget
 * (ADR-0004) already spent. Where a CLI splits a window by model group (agy),
 * the ring shows the fullest group and the tooltip lists them all.
 */
export function UsagePanel({ runs, windows }: Props) {
  const budget = useBudget();
  const all = [...windows, ...latestStreamQuotas(runs)];
  const providers = ORDER.filter((p) => runs.some((r) => r.cards.some((c) => c.provider === p)) || all.some((w) => w.provider === p));
  if (providers.length === 0) return null;

  const meta = `고리 숫자는 쓴 % · 토큰은 작업 ${runs.length}개 합계${budget ? ` · 오늘 예산은 주간의 ${budget.percent}%` : ""}`;
  return (
    <section aria-labelledby="usage-title" className="px-5">
      <h3 id="usage-title" className="mb-2 flex min-w-0 items-baseline gap-2 text-2xs font-semibold tracking-wider text-muted-foreground uppercase">
        <span className="shrink-0">에이전트 사용량</span>
        <span className="min-w-0 truncate font-normal tracking-normal normal-case" title={meta}>
          {meta}
        </span>
      </h3>
      <ul className="grid auto-cols-fr grid-flow-col divide-x rounded-xl border bg-card text-card-foreground">
        {providers.map((provider) => {
          const cards = runs.flatMap((r) => r.cards.filter((c) => c.provider === provider));
          const tokens = cards.reduce((sum, c) => sum + tokensOf(c), 0);
          const mine = all.filter((w) => w.provider === provider);
          return (
            <li key={provider} className="flex min-w-0 flex-col gap-1.5 px-2.5 py-2">
              <span className="flex min-w-0 items-center gap-1.5">
                <Avatar speaker={provider} size="sm" />
                <span className="min-w-0 flex-1">
                  <span className="block truncate text-xs font-medium" title={PROVIDER_NAME[provider]}>
                    {PROVIDER_NAME[provider]}
                  </span>
                  <span className="block truncate text-3xs text-muted-foreground tabular-nums">{tokens > 0 ? `${compact.format(tokens)} 토큰` : `카드 ${cards.length}장`}</span>
                </span>
              </span>
              {/* Each ring names its window underneath: the number inside is the share used. */}
              <span className="flex gap-1.5">
                {RINGS.map((ring) => (
                  <Captioned key={ring.window} caption={ring.name}>
                    <Ring name={`${PROVIDER_NAME[provider]} ${ring.name}`} windows={mine.filter((w) => w.window === ring.window)} />
                  </Captioned>
                ))}
                {budget && (
                  <Captioned caption="오늘">
                    <TodayRing name={`${PROVIDER_NAME[provider]} 오늘 예산`} budget={budget.providers.find((b) => b.provider === provider)} />
                  </Captioned>
                )}
              </span>
            </li>
          );
        })}
      </ul>
    </section>
  );
}

const SIZE = 26;
const R = 10.5;
const LENGTH = 2 * Math.PI * R;

/** A ring with what it measures written under it. */
function Captioned({ caption, children }: { caption: string; children: ReactNode }) {
  return (
    <span className="flex flex-col items-center gap-0.5">
      {children}
      <span aria-hidden="true" className="text-3xs leading-none whitespace-nowrap text-muted-foreground">
        {caption}
      </span>
    </span>
  );
}

/** A dashed ring: the CLI has not told us this one. */
function EmptyRing({ title, name }: { title: string; name: string }) {
  return (
    <span title={title} className="relative grid size-[26px] place-items-center text-3xs text-muted-foreground">
      <svg viewBox={`0 0 ${SIZE} ${SIZE}`} className="absolute inset-0" aria-hidden="true">
        <circle cx={13} cy={13} r={R} fill="none" strokeWidth={2.5} strokeDasharray="2 3" className="stroke-muted" />
      </svg>
      <span className="sr-only">{name} 알 수 없음</span>
      <span aria-hidden="true">–</span>
    </span>
  );
}

/** Activity ring: an arc for `fraction` (0 to 1) in `tone`, the percent in the middle. */
function Meter({ fraction, tone, name, title }: { fraction: number; tone: string; name: string; title: string }) {
  const percent = Math.round(fraction * 100);
  return (
    <span
      role="meter"
      aria-label={name}
      aria-valuenow={percent}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuetext={`${percent}%`}
      title={title}
      className="relative grid size-[26px] place-items-center text-3xs tabular-nums"
    >
      <svg viewBox={`0 0 ${SIZE} ${SIZE}`} className="absolute inset-0 -rotate-90" aria-hidden="true">
        <circle cx={13} cy={13} r={R} fill="none" strokeWidth={2.5} className="stroke-muted" />
        <circle cx={13} cy={13} r={R} fill="none" strokeWidth={2.5} strokeLinecap="round" strokeDasharray={`${(LENGTH * Math.min(1, fraction)).toFixed(2)} ${LENGTH.toFixed(2)}`} className={tone} />
      </svg>
      <span aria-hidden="true">{percent}</span>
    </span>
  );
}

/** A subscription window: one arc, fuller is warmer. */
function Ring({ name, windows }: { name: string; windows: UsageWindow[] }) {
  const worst = windows.reduce<UsageWindow | undefined>((w, x) => (!w || x.used > w.used ? x : w), undefined);
  if (!worst) return <EmptyRing name={name} title={`${name}: CLI가 알려 주지 않음`} />;
  const tone = worst.used >= 0.9 ? "stroke-red-500" : worst.used >= 0.7 ? "stroke-amber-500" : "stroke-emerald-500";
  const detail = windows
    .map((w) => `${w.group ? `${w.group} ` : ""}${Math.round(w.used * 100)}% · ${timeOf(w.resetsAt)} 초기화`)
    .join("\n");
  return <Meter fraction={worst.used} tone={tone} name={`${name} 사용`} title={`${name}\n${detail}`} />;
}

/** The ring's colour says what the runner does now: keeps effort, lowers it, or sends no new cards. */
const TODAY_TONE: Record<ProviderBudget["state"], string> = {
  unknown: "stroke-muted",
  ok: "stroke-emerald-500",
  behind: "stroke-amber-500",
  farBehind: "stroke-amber-500",
  over: "stroke-red-500",
};

/** Today's spend against the day's allowance (percent points of the weekly window). */
function TodayRing({ name, budget }: { name: string; budget: ProviderBudget | undefined }) {
  if (!budget || budget.spent === null) return <EmptyRing name={name} title={`${name}: 주간 사용량 기록이 없어 조절하지 않음`} />;
  const lines = [
    `${name}: 주간의 ${budget.spent}%p / ${budget.allowance}%p 사용`,
    budget.remainingHours !== null ? `남은 시간 ${budget.remainingHours}시간` : "",
    budget.state === "over"
      ? "새 작업을 보내지 않음"
      : budget.efforts.length > 0
        ? `effort ${budget.stepsDown}단계 낮춤: ${budget.efforts.map((e) => `${e.from ?? "기본"}→${e.to}`).join(", ")}`
        : "effort 그대로",
    budget.readAt ? `${timeOf(budget.readAt)} 기록 기준` : "",
  ];
  return <Meter fraction={budget.spent / budget.allowance} tone={TODAY_TONE[budget.state]} name={`${name} 사용`} title={lines.filter(Boolean).join("\n")} />;
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

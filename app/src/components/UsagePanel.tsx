import type { ProviderId } from "../../../src/core/plan.ts";
import type { Quota, RunView } from "../../../src/core/snapshot.ts";
import { PROVIDER_NAME, timeOf } from "../format.ts";
import { Avatar } from "./Avatar.tsx";

const compact = new Intl.NumberFormat("ko-KR", { notation: "compact", maximumFractionDigits: 1 });
const WINDOW_NAME: Record<string, string> = { five_hour: "5시간", seven_day: "주간" };

/**
 * What each agent in use has spent, only as its CLI reported it: tokens for
 * every CLI, subscription windows where the CLI tells us (Claude does).
 */
export function UsagePanel({ runs }: { runs: RunView[] }) {
  const providers = [...new Set(runs.flatMap((r) => r.cards.map((c) => c.provider)))];
  if (providers.length === 0) return null;
  const quotas = latestQuotas(runs);

  return (
    <section aria-labelledby="usage-title" className="px-5">
      <h3 id="usage-title" className="mb-2 flex items-baseline gap-2 text-2xs font-semibold tracking-wider text-muted-foreground uppercase">
        에이전트 사용량
        <span className="font-normal tracking-normal normal-case">작업 {runs.length}개 합계, 각 CLI가 보고한 값</span>
      </h3>
      <ul className="divide-y rounded-xl border bg-card text-card-foreground">
        {providers.map((provider) => {
          const cards = runs.flatMap((r) => r.cards.filter((c) => c.provider === provider));
          const total = cards.reduce((sum, c) => ({ input: sum.input + c.usage.input, cached: sum.cached + c.usage.cached, output: sum.output + c.usage.output }), { input: 0, cached: 0, output: 0 });
          const windows = quotas.filter((q) => q.provider === provider);
          const reported = cards.some((c) => c.usage.input + c.usage.cached + c.usage.output > 0);
          return (
            <li key={provider} className="flex items-start gap-3 px-3.5 py-3">
              <Avatar speaker={provider} size="sm" />
              <div className="min-w-0 flex-1">
                <p className="flex items-baseline gap-2 text-sm">
                  <span className="font-medium">{PROVIDER_NAME[provider]}</span>
                  <span className="text-xs text-muted-foreground">카드 {cards.length}장</span>
                </p>
                <p className="mt-0.5 text-xs text-muted-foreground tabular-nums">
                  {reported ? (
                    <>
                      입력 <span className="text-foreground">{compact.format(total.input)}</span> · 캐시 {compact.format(total.cached)} · 출력{" "}
                      <span className="text-foreground">{compact.format(total.output)}</span> 토큰
                    </>
                  ) : (
                    "토큰 사용량을 아직 보고받지 못했습니다"
                  )}
                </p>
                {windows.length > 0 ? (
                  <div className="mt-2 grid gap-1.5">
                    {windows.map((q) => (
                      <QuotaBar key={q.window} quota={q} />
                    ))}
                  </div>
                ) : (
                  <p className="mt-1 text-2xs text-muted-foreground">구독 한도 사용률: {PROVIDER_NAME[provider]} CLI가 알려 주지 않음</p>
                )}
              </div>
            </li>
          );
        })}
      </ul>
    </section>
  );
}

function QuotaBar({ quota }: { quota: Quota }) {
  const percent = Math.round(quota.used * 100);
  const tone = quota.used >= 0.9 ? "bg-red-500" : quota.used >= 0.7 ? "bg-amber-500" : "bg-emerald-500";
  const label = `${WINDOW_NAME[quota.window] ?? quota.window} ${percent}%`;
  return (
    <div className="grid grid-cols-[4.5rem_minmax(0,1fr)_auto] items-center gap-2 text-2xs text-muted-foreground">
      <span>{label}</span>
      <span role="meter" aria-label={`${label} 사용`} aria-valuenow={percent} aria-valuemin={0} aria-valuemax={100} className="h-1.5 overflow-hidden rounded-full bg-muted">
        <span className={`block h-full rounded-full ${tone}`} style={{ width: `${Math.min(100, percent)}%` }} />
      </span>
      <span className="tabular-nums">{timeOf(quota.resetsAt)} 초기화</span>
    </div>
  );
}

/** The newest report of each provider's windows across the runs shown. */
function latestQuotas(runs: RunView[]): Quota[] {
  const latest = new Map<string, Quota>();
  for (const q of runs.flatMap((r) => r.quotas)) {
    const key = `${q.provider}/${q.window}`;
    const seen = latest.get(key);
    if (!seen || seen.at < q.at) latest.set(key, q);
  }
  const order: ProviderId[] = ["claude", "codex", "agy"];
  return [...latest.values()].sort((a, b) => order.indexOf(a.provider) - order.indexOf(b.provider) || a.window.localeCompare(b.window));
}

import { CircleCheck, CircleX, GitPullRequest, MessageSquare, Play, Rocket } from "lucide-react";
import { useState } from "react";

import { activity, fleetStats, type ActivityItem, type NextItem } from "../../../src/core/attention.ts";
import type { CardView, RunView } from "../../../src/core/snapshot.ts";
import { PROVIDER_SHORT, STATE_WORD, clip, timeOf } from "../format.ts";
import { NextTicket } from "./NextUp.tsx";
import { age } from "./Ticket.tsx";

const compact = new Intl.NumberFormat("ko-KR", { notation: "compact", maximumFractionDigits: 1 });
/** Kinds that hold an agent or a person up; the pass shows these first. */
const URGENT = new Set(["answer", "failed", "silent", "limit", "review"]);
const PASS_VISIBLE = 4;

interface Props {
  runs: RunView[];
  todo: NextItem[];
  stopped: NextItem[];
  now: Date;
  onOpen: (item: NextItem) => void;
  onToggle: (key: string) => void;
  onNextUp: () => void;
  onBoard: () => void;
  onCard: (run: string, card: string) => void;
}

/**
 * The Orchestrator dashboard's order for one O project: who is waiting on you
 * first, then how the work is moving (strip, flow, activity).
 */
export function WorkDashboard({ runs, todo, stopped, now, onOpen, onToggle, onNextUp, onBoard, onCard }: Props) {
  const [all, setAll] = useState(false);
  const pass = [...todo, ...stopped].filter((i) => URGENT.has(i.kind)).sort((a, b) => a.since.localeCompare(b.since));
  const shown = all ? pass : pass.slice(0, PASS_VISIBLE);
  const stats = fleetStats(runs, now);

  return (
    <div className="flex flex-col gap-5">
      <section aria-labelledby="pass-title">
        <header className="mb-2.5 flex items-baseline gap-2 border-b-2 border-muted-foreground/40 pb-1.5">
          <h3 id="pass-title" className="text-sm font-bold">
            기다리는 일
          </h3>
          {pass.length > 0 && <span className="rounded-full bg-red-500 px-1.5 text-2xs font-bold text-white tabular-nums">{pass.length}</span>}
          <span className="truncate text-2xs text-muted-foreground">오래 기다린 것부터</span>
          {pass.length > PASS_VISIBLE && (
            <button type="button" onClick={() => setAll((a) => !a)} className="ml-auto shrink-0 rounded px-1.5 text-2xs text-muted-foreground hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none">
              {all ? "줄여 보기" : `모두 ${pass.length}개 보기`}
            </button>
          )}
        </header>
        {pass.length === 0 ? (
          <p className="rounded-lg border border-dashed px-4 py-5 text-center text-xs text-muted-foreground">
            지금 기다리는 일이 없습니다. 에이전트의 질문, 실패·응답 없는 카드, 검토할 PR이 생기면 여기에 먼저 걸립니다.
          </p>
        ) : (
          <div className="@container">
            <div className="grid gap-2.5 @xl:grid-cols-2">
              {shown.map((item) => (
                <NextTicket key={item.key} item={item} now={now} onOpen={onOpen} onDone={onToggle} />
              ))}
            </div>
          </div>
        )}
      </section>

      <div className="@container">
        <div className="grid grid-cols-2 gap-px overflow-hidden rounded-xl border bg-border text-card-foreground @xl:grid-cols-4" role="group" aria-label="작업 현황">
          <Metric label="작업 중" value={stats.running} hint={`${runs.filter((r) => r.status === "running").length}개 실행`} tone="text-agent-working" onClick={onBoard} action="보드 열기" />
          <Metric label="멈춤" value={stopped.length} hint={`실패·중지 ${stats.stopped}`} tone={stopped.length > 0 ? "text-red-400" : "text-muted-foreground"} onClick={onNextUp} action="다음 할 일 열기" />
          <Metric label="PR" value={stats.prs} hint="에이전트가 보고한 PR" tone="text-sky-400" onClick={onNextUp} action="다음 할 일 열기" />
          <Metric label="오늘 카드" value={stats.cardsToday} hint={stats.tokensToday > 0 ? `${compact.format(stats.tokensToday)} 토큰` : "토큰 보고 없음"} />
        </div>
      </div>

      <WorkFlow runs={runs} now={now} onCard={onCard} />
      <Activity items={activity(runs, 10)} now={now} onCard={onCard} />
    </div>
  );
}

function Metric({ label, value, hint, tone = "", onClick, action }: { label: string; value: number; hint: string; tone?: string; onClick?: () => void; action?: string }) {
  const body = (
    <>
      <span className={`text-2xl leading-none font-bold tabular-nums ${tone}`}>{value}</span>
      <span className="min-w-0">
        <span className="block text-xs font-semibold">{label}</span>
        <span className="block truncate text-2xs text-muted-foreground">{hint}</span>
      </span>
    </>
  );
  const cls = "flex items-center gap-2.5 bg-card px-3.5 py-3 text-left";
  return onClick ? (
    <button type="button" onClick={onClick} title={action} className={`${cls} hover:bg-accent/40 focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none focus-visible:ring-inset`}>
      {body}
    </button>
  ) : (
    <div className={cls}>{body}</div>
  );
}

const BAR: Record<CardView["state"], string> = {
  waiting: "bg-neutral-500/50",
  queued: "bg-neutral-500/70",
  running: "bg-agent-working",
  done: "bg-emerald-500/80",
  failed: "bg-red-500/80",
  stopped: "bg-neutral-400/60",
};
const FLOW_CARDS = 10;
/** Quiet stretches longer than this fold into a thin gap, so minute-long cards hours apart stay readable. */
const FOLD_MS = 10 * 60_000;
const GAP_SHARE = 3;

/** A time axis that skips idle gaps: busy stretches share the width by length, each gap gets a sliver. */
function timeScale(spans: (readonly [number, number])[]) {
  const sorted = [...spans].sort((a, b) => a[0] - b[0]);
  const merged: [number, number][] = [];
  for (const [s, e] of sorted) {
    const last = merged.at(-1);
    if (last && s - last[1] <= FOLD_MS) last[1] = Math.max(last[1], e);
    else merged.push([s, Math.max(e, s + 1000)]);
  }
  const room = 100 - GAP_SHARE * (merged.length - 1);
  const total = merged.reduce((sum, [s, e]) => sum + (e - s), 0);
  let left = 0;
  const stretches = merged.map(([from, to]) => {
    const width = (room * (to - from)) / total;
    const stretch = { from, to, left, width };
    left += width + GAP_SHARE;
    return stretch;
  });
  const at = (t: number) => {
    const s = stretches.find((x) => t <= x.to) ?? stretches[stretches.length - 1];
    return s.left + (Math.min(Math.max(t, s.from), s.to) - s.from) / (s.to - s.from) * s.width;
  };
  return { at, stretches };
}

/** The Orchestrator's commit flow, as card lanes: when each card ran, and how it ended. */
function WorkFlow({ runs, now, onCard }: { runs: RunView[]; now: Date; onCard: (run: string, card: string) => void }) {
  const lanes = runs
    .flatMap((run) => run.cards.filter((c) => c.startedAt).map((card) => ({ run, card })))
    .sort((a, b) => (b.card.startedAt ?? "").localeCompare(a.card.startedAt ?? ""))
    .slice(0, FLOW_CARDS)
    .reverse();
  if (lanes.length === 0) return null;
  const spans = lanes.map((l) => [Date.parse(l.card.startedAt!), Date.parse(l.card.endedAt ?? now.toISOString())] as const);
  const { at, stretches } = timeScale(spans);
  return (
    <section aria-labelledby="flow-title" className="rounded-xl border bg-card p-3.5 text-card-foreground">
      <h3 id="flow-title" className="flex items-baseline gap-2 text-2xs font-semibold tracking-wider text-muted-foreground uppercase">
        작업 흐름
        <span className="font-normal tracking-normal normal-case">최근 카드 {lanes.length}장 · 쉬는 시간은 접어 둠</span>
      </h3>
      <div className="mt-2 grid grid-cols-[minmax(0,7.5rem)_minmax(0,1fr)] gap-2" aria-hidden="true">
        <span />
        <span className="relative h-4 text-3xs text-muted-foreground tabular-nums">
          {stretches.map((s) => (
            <span key={s.from} className="absolute top-0 border-l border-muted-foreground/40 pl-1 whitespace-nowrap" style={{ left: `${s.left}%` }}>
              {timeOf(new Date(s.from).toISOString())}
            </span>
          ))}
        </span>
      </div>
      <ol className="mt-1 grid gap-1.5">
        {lanes.map(({ run, card }, i) => {
          const from = at(spans[i][0]);
          const to = at(spans[i][1]);
          return (
            <li key={`${run.runId}/${card.id}`} className="grid grid-cols-[minmax(0,7.5rem)_minmax(0,1fr)] items-center gap-2">
              <button
                type="button"
                onClick={() => onCard(run.runId, card.id)}
                className="truncate rounded text-left text-2xs hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
                title={`${run.title} · ${card.label}`}
              >
                <span className="text-foreground">{card.label}</span> <span className="text-muted-foreground">{PROVIDER_SHORT[card.provider]}</span>
              </button>
              <span className="relative h-3 rounded-sm bg-muted/50">
                <span
                  className={`absolute inset-y-0 min-w-1 rounded-sm ${BAR[card.state]}`}
                  style={{ left: `${from}%`, width: `${Math.max(0.8, to - from)}%` }}
                  title={`${card.label}: ${STATE_WORD[card.state]} ${timeOf(card.startedAt)}–${timeOf(card.endedAt) || "진행 중"}`}
                />
                <span className="sr-only">
                  {STATE_WORD[card.state]}, {timeOf(card.startedAt)}부터 {card.endedAt ? `${timeOf(card.endedAt)}까지` : "진행 중"}
                </span>
              </span>
            </li>
          );
        })}
      </ol>
    </section>
  );
}

const ICON: Record<ActivityItem["kind"], typeof Play> = { run: Rocket, start: Play, end: CircleCheck, msg: MessageSquare, pr: GitPullRequest };

function Activity({ items, now, onCard }: { items: ActivityItem[]; now: Date; onCard: (run: string, card: string) => void }) {
  if (items.length === 0) return null;
  return (
    <section aria-labelledby="activity-title" className="rounded-xl border bg-card text-card-foreground">
      <h3 id="activity-title" className="flex items-baseline gap-2 px-3.5 pt-3 pb-1 text-2xs font-semibold tracking-wider text-muted-foreground uppercase">
        활동 <span className="font-normal tracking-normal normal-case">최근 {items.length}개</span>
      </h3>
      <ul className="divide-y">
        {items.map((item) => {
          const Icon = item.kind === "end" && /실패|중지/.test(item.detail) ? CircleX : ICON[item.kind];
          const body = (
            <>
              <Icon className="mt-0.5 size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
              <span className="min-w-0 flex-1">
                <span className="block truncate text-xs">{clip(item.text, 90)}</span>
                <span className="block truncate text-2xs text-muted-foreground">
                  {item.detail}
                  {item.provider && ` · ${PROVIDER_SHORT[item.provider]}`}
                </span>
              </span>
              <span className="shrink-0 text-2xs text-muted-foreground tabular-nums">{age(item.at, now)} 전</span>
            </>
          );
          const cardId = item.cardId;
          return (
            <li key={`${item.kind}/${item.runId}/${item.cardId ?? ""}/${item.at}`}>
              {cardId ? (
                <button type="button" onClick={() => onCard(item.runId, cardId)} className="flex w-full items-start gap-2.5 px-3.5 py-2 text-left hover:bg-accent/40 focus-visible:bg-accent focus-visible:outline-none">
                  {body}
                </button>
              ) : (
                <div className="flex items-start gap-2.5 px-3.5 py-2">{body}</div>
              )}
            </li>
          );
        })}
      </ul>
    </section>
  );
}

import { ArrowLeft, ClipboardList, Columns3, KanbanSquare, LayoutDashboard, ListTodo, type LucideIcon } from "lucide-react";
import { useEffect, useMemo, useState, type ReactNode } from "react";

import { nextUp, type NextItem } from "../../../../src/core/attention.ts";
import type { RunView } from "../../../../src/core/snapshot.ts";
import { names } from "../../lib/format.ts";
import type { LeadState } from "../../data/useLead.ts";
import { readStored, writeStored } from "../../lib/storage.ts";
import type { PanelViewDef } from "../../shell/SidePanel.tsx";
import { CardTranscript } from "../chat/CardTranscript.tsx";
import { PlanCard } from "../chat/LeadThread.tsx";
import { Markdown } from "../chat/Markdown.tsx";
import { AgentBoard } from "./AgentBoard.tsx";
import { NextUp, actionCount, useHandled } from "./NextUp.tsx";
import { ReviewRequestForm } from "./ReviewRequest.tsx";
import { WorkDashboard } from "./WorkDashboard.tsx";
import { DesignAnnotations } from "./DesignAnnotations.tsx";

/** The side panel's "에이전트 작업" view. */
export const WORK_VIEW: PanelViewDef = {
  label: "에이전트 작업",
  icon: KanbanSquare,
  render: ({ run, projectRuns, card, now, onCard, lead }) => <WorkTab runs={projectRuns} open={run && card ? { run: run.runId, card } : undefined} onCard={onCard} now={now} lead={lead} />,
};

type Sub = "design" | "dashboard" | "next" | "board";

/** What every sub-view of the work tab is drawn from. */
interface SubContext {
  runs: RunView[];
  lead?: LeadState;
  todo: NextItem[];
  stopped: NextItem[];
  now: Date;
  handled: ReturnType<typeof useHandled>["handled"];
  toggle: (key: string) => void;
  openItem: (item: NextItem) => void;
  choose: (sub: Sub) => void;
  onCard: (target?: { run: string; card: string }) => void;
}

interface SubDef {
  id: Sub;
  label: string;
  icon: LucideIcon;
  /** A count on the button (the next view's waiting items); nothing when 0. */
  badge?: (ctx: SubContext) => number;
  /** Shows the "에이전트 검토 요청" form above the view. */
  reviewRequest?: boolean;
  render: (ctx: SubContext) => ReactNode;
}

/** The work tab's sub-views, in button order; the first is the default. */
const SUBS: SubDef[] = [
  {
    id: "design",
    label: "설계",
    icon: ClipboardList,
    render: ({ lead, runs, onCard }) => <DesignPanel lead={lead} runs={runs} onCard={onCard} />,
  },
  {
    id: "dashboard",
    label: "대시보드",
    icon: LayoutDashboard,
    render: (c) => (
      <WorkDashboard
        runs={c.runs}
        todo={c.todo}
        stopped={c.stopped}
        now={c.now}
        onOpen={c.openItem}
        onToggle={c.toggle}
        onNextUp={() => c.choose("next")}
        onBoard={() => c.choose("board")}
        onCard={(run, card) => c.onCard({ run, card })}
      />
    ),
  },
  {
    id: "next",
    label: "다음 할 일",
    icon: ListTodo,
    // What waits on a person: notes to read and Claude Code sessions' tickets are not counted.
    badge: (c) => actionCount([...c.todo, ...c.stopped]),
    reviewRequest: true,
    render: (c) => <NextUp todo={c.todo} stopped={c.stopped} now={c.now} handled={c.handled} onToggle={c.toggle} onOpen={c.openItem} />,
  },
  { id: "board", label: "보드", icon: Columns3, reviewRequest: true, render: (c) => <AgentBoard runs={c.runs} onOpen={c.onCard} /> },
];
const SUB_KEY = "project-o-work-tab";

const loadSub = () => readStored<Sub>(SUB_KEY, "design", (saved) => SUBS.find((s) => s.id === saved)?.id);

function DesignPanel({ lead, runs, onCard }: { lead?: LeadState; runs: RunView[]; onCard: Props["onCard"] }) {
  if (!lead) return <p className="text-sm text-muted-foreground">실제 프로젝트의 총괄 대화에서 설계를 확인할 수 있습니다.</p>;
  if (lead.failed) return <p role="alert" className="text-sm text-destructive">설계를 읽지 못했습니다. <button type="button" className="underline focus-ring" onClick={() => void lead.refresh()}>다시 시도</button></p>;
  if (!lead.loaded) return <p role="status" className="text-sm text-muted-foreground">설계를 불러오는 중…</p>;
  return <DesignContent key={lead.thread?.threadId} lead={lead} runs={runs} onCard={onCard} />;
}

function DesignContent({ lead, runs, onCard }: { lead: LeadState; runs: RunView[]; onCard: Props["onCard"] }) {
  const events = lead.thread?.events ?? [];
  const designIndex = events.findLastIndex((event) => event.t === "lead.design");
  const event = events[designIndex];
  const design = event?.t === "lead.design" ? event.design : undefined;
  const earlier = events.slice(0, designIndex).findLast((item) => item.t === "lead.design");
  const old = earlier?.t === "lead.design" ? earlier.design : undefined;
  const removed = old && design && Array.isArray(old.decisions) && Array.isArray(old.questions) && Array.isArray(design.decisions) && Array.isArray(design.questions) ? [old.goal, ...old.decisions, ...old.questions].filter((line) => line && ![design.goal, ...design.decisions, ...design.questions].includes(line)) : [];
  const designStale = designIndex >= 0 && events.slice(designIndex + 1).some((item) => item.t === "lead.user");
  const summary = design && typeof design.goal === "string" && Array.isArray(design.decisions) && Array.isArray(design.questions) && (
    <section key={event?.at} aria-label="설계 요약" className="design-rewrite space-y-3 border-b pb-4 text-sm">
      <DesignChanges revision={event.at} removed={removed} />
      <div><h3 className="text-xs text-muted-foreground">목표</h3><p className="mt-1 whitespace-pre-wrap">{design.goal || "아직 정하지 않았습니다."}</p></div>
      {[["결정", design.decisions], ["미결정", design.questions]].map(([label, entries]) => (
        <div key={label as string}><h3 className="text-xs text-muted-foreground">{label}</h3><ul className="mt-1 list-disc space-y-1 pl-4">{(entries as string[]).filter((entry) => typeof entry === "string").map((entry, i) => <li key={i} className="whitespace-pre-wrap">{entry}</li>)}</ul></div>
      ))}
      {designStale && <p role="status" className="text-xs text-status-warning-text">이후 대화가 있어 설계가 최신이 아닐 수 있습니다.</p>}
      {removed.length > 0 && <details className="text-xs text-muted-foreground"><summary className="cursor-pointer focus-ring">이전 설계에서 바뀐 내용 {removed.length}개</summary><ul className="mt-1 space-y-1">{removed.map((line, i) => <li key={i}><del>{line}</del></li>)}</ul></details>}
    </section>
  );
  const plans = lead.items.filter((item) => item.kind === "plan");
  const latest = plans.at(-1);
  if (!latest) return <DesignAnnotations lead={lead}>{summary}<p className="text-sm text-muted-foreground">총괄과 작업을 논의하면 제안된 작업이 여기에 나타납니다.</p></DesignAnnotations>;
  const previous = lead.items[lead.items.indexOf(latest) - 1];
  return (
    <DesignAnnotations lead={lead}>
      <div><h2 className="text-sm font-medium">현재 설계</h2><p className="mt-1 text-xs text-muted-foreground">총괄의 최신 제안을 확인하고 작업을 분배하세요.</p></div>
      {summary}
      {previous?.kind === "reply" && <details className="text-sm"><summary className="cursor-pointer text-xs text-muted-foreground focus-ring">총괄 설명과 미결정 사항</summary><div className="mt-2"><Markdown text={previous.text} /></div></details>}
      <PlanCard plan={latest} lead={lead} run={runs.find((run) => run.runId === latest.runId)} onOpenCard={(run, card) => onCard({ run, card })} />
      {plans.length > 1 && <details className="text-xs text-muted-foreground"><summary className="cursor-pointer focus-ring">이전 설계 {plans.length - 1}개</summary><ol className="mt-2 space-y-1 pl-4">{plans.slice(0, -1).reverse().map((plan) => <li key={plan.planId}>{plan.title}</li>)}</ol></details>}
    </DesignAnnotations>
  );
}

function DesignChanges({ revision, removed }: { revision: string; removed: string[] }) {
  const [visible, setVisible] = useState(removed.length > 0);
  useEffect(() => {
    if (!visible) return;
    const timer = window.setTimeout(() => setVisible(false), 1800);
    return () => window.clearTimeout(timer);
  }, [revision, visible]);
  return visible && removed.length > 0 ? <div className="design-removed text-xs text-muted-foreground" role="status"><p>이전 내용 수정 중</p>{removed.map((line, i) => <p key={i} className="break-words"><del>{line}</del></p>)}</div> : null;
}

/** The clock tickets age by: the caller's (a replay's) or the wall clock, re-read every half minute. */
export function useNow(given: Date | undefined): Date {
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    if (given) return;
    const id = window.setInterval(() => setNow(new Date()), 30_000);
    return () => window.clearInterval(id);
  }, [given]);
  return given ?? now;
}

interface Props {
  runs: RunView[];
  /** The opened card's record, if any; it covers the sub-views until closed. */
  open?: { run: string; card: string };
  onCard: (target?: { run: string; card: string }) => void;
  now?: Date;
  lead?: LeadState;
}

/**
 * The agent work tab: the Orchestrator app's dashboard and next up brought to
 * one O project, with the card board beside them.
 */
export function WorkTab({ runs, open, onCard, now: given, lead }: Props) {
  const [sub, setSub] = useState<Sub>(loadSub);
  const now = useNow(given);
  const { handled, toggle } = useHandled();
  const { todo, stopped } = useMemo(() => nextUp(runs, now, handled), [runs, now, handled]);
  const choose = (next: Sub) => {
    setSub(next);
    writeStored(SUB_KEY, next);
  };
  const openItem = (item: NextItem) => {
    const run = runs.find((r) => r.runId === item.runId);
    const card = item.cardId ?? run?.cards[0]?.id;
    if (card) onCard({ run: item.runId, card });
  };
  const current = SUBS.find((s) => s.id === sub) ?? SUBS[0];

  const opened = open && runs.find((r) => r.runId === open.run);
  const openedCard = opened?.cards.find((c) => c.id === open?.card);
  if (opened && openedCard) {
    return (
      <div className="flex min-h-0 flex-1 flex-col">
        <button
          type="button"
          onClick={() => onCard(undefined)}
          className="flex h-8 shrink-0 items-center gap-1.5 border-b px-3 text-xs text-muted-foreground hover:bg-accent/40 hover:text-foreground focus-ring focus-visible:ring-inset"
        >
          <ArrowLeft className="size-3.5" aria-hidden="true" />
          {current.label}(으)로 돌아가기
          <span className="truncate">{opened.title}</span>
        </button>
        <CardTranscript card={openedCard} run={opened} names={names(opened)} />
      </div>
    );
  }

  const ctx: SubContext = { runs, lead, todo, stopped, now, handled, toggle, openItem, choose, onCard };
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div role="group" aria-label="에이전트 작업 보기" className="flex shrink-0 gap-1 border-b px-3 py-1.5">
        {SUBS.map((s) => {
          const count = s.badge?.(ctx) ?? 0;
          return (
            <button
              key={s.id}
              type="button"
              aria-pressed={sub === s.id}
              onClick={() => choose(s.id)}
              className="flex h-7 items-center gap-1.5 rounded-md px-2.5 text-xs text-muted-foreground hover:bg-accent/50 hover:text-foreground focus-ring aria-pressed:bg-accent aria-pressed:text-foreground"
            >
              <s.icon className="size-3.5" aria-hidden="true" />
              {s.label}
              {count > 0 && (
                <span className="rounded-full bg-destructive px-1.5 text-3xs font-bold text-destructive-foreground tabular-nums">
                  {count}
                  <span className="sr-only">건</span>
                </span>
              )}
            </button>
          );
        })}
      </div>
      <div className="scrollbar-sleek min-h-0 flex-1 overflow-auto bg-chat-canvas p-4">
        {runs.length === 0 && current.id !== "design" ? (
          <p className="text-sm text-muted-foreground">이 프로젝트에는 아직 작업이 없습니다.</p>
        ) : (
          <>
            {/* Every run here shares one project folder: a person's PR can be handed to an agent from it. */}
            {current.reviewRequest && runs[0] && <ReviewRequestForm project={runs[0].project} />}
            {current.render(ctx)}
          </>
        )}
      </div>
    </div>
  );
}

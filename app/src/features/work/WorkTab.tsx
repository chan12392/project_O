import { ArrowLeft, Columns3, KanbanSquare, LayoutDashboard, ListTodo, type LucideIcon } from "lucide-react";
import { useEffect, useMemo, useState, type ReactNode } from "react";

import { nextUp, type NextItem } from "../../../../src/core/attention.ts";
import type { RunView } from "../../../../src/core/snapshot.ts";
import { names } from "../../lib/format.ts";
import { readStored, writeStored } from "../../lib/storage.ts";
import type { PanelViewDef } from "../../shell/SidePanel.tsx";
import { CardTranscript } from "../chat/CardTranscript.tsx";
import { AgentBoard } from "./AgentBoard.tsx";
import { NextUp, actionCount, useHandled } from "./NextUp.tsx";
import { ReviewRequestForm } from "./ReviewRequest.tsx";
import { WorkDashboard } from "./WorkDashboard.tsx";

/** The side panel's "에이전트 작업" view. */
export const WORK_VIEW: PanelViewDef = {
  label: "에이전트 작업",
  icon: KanbanSquare,
  render: ({ run, projectRuns, card, now, onCard }) => <WorkTab runs={projectRuns} open={run && card ? { run: run.runId, card } : undefined} onCard={onCard} now={now} />,
};

type Sub = "dashboard" | "next" | "board";

/** What every sub-view of the work tab is drawn from. */
interface SubContext {
  runs: RunView[];
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

const loadSub = () => readStored<Sub>(SUB_KEY, "dashboard", (saved) => SUBS.find((s) => s.id === saved)?.id);

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
}

/**
 * The agent work tab: the Orchestrator app's dashboard and next up brought to
 * one O project, with the card board beside them.
 */
export function WorkTab({ runs, open, onCard, now: given }: Props) {
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

  const ctx: SubContext = { runs, todo, stopped, now, handled, toggle, openItem, choose, onCard };
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
        {runs.length === 0 ? (
          <p className="text-sm text-muted-foreground">이 프로젝트에는 아직 작업이 없습니다.</p>
        ) : (
          <>
            {/* Every run here shares one project folder: a person's PR can be handed to an agent from it. */}
            {current.reviewRequest && <ReviewRequestForm project={runs[0].project} />}
            {current.render(ctx)}
          </>
        )}
      </div>
    </div>
  );
}

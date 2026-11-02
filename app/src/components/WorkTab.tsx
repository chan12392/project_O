import { ArrowLeft, Columns3, LayoutDashboard, ListTodo } from "lucide-react";
import { useEffect, useMemo, useState } from "react";

import { nextUp, type NextItem } from "../../../src/core/attention.ts";
import type { RunView } from "../../../src/core/snapshot.ts";
import { names } from "../format.ts";
import { readStored, writeStored } from "../storage.ts";
import { AgentBoard } from "./AgentBoard.tsx";
import { CardTranscript } from "./CardTranscript.tsx";
import { NextUp, useHandled } from "./NextUp.tsx";
import { WorkDashboard } from "./WorkDashboard.tsx";

type Sub = "dashboard" | "next" | "board";
const SUBS: { id: Sub; label: string; icon: typeof ListTodo }[] = [
  { id: "dashboard", label: "대시보드", icon: LayoutDashboard },
  { id: "next", label: "다음 할 일", icon: ListTodo },
  { id: "board", label: "보드", icon: Columns3 },
];
const SUB_KEY = "project-o-work-tab";

const loadSub = () => readStored<Sub>(SUB_KEY, "dashboard", (saved) => (saved === "next" || saved === "board" ? saved : undefined));

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
          {SUBS.find((s) => s.id === sub)?.label}(으)로 돌아가기
          <span className="truncate">{opened.title}</span>
        </button>
        <CardTranscript card={openedCard} run={opened} names={names(opened)} />
      </div>
    );
  }

  const waiting = todo.length + stopped.length;
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div role="group" aria-label="에이전트 작업 보기" className="flex shrink-0 gap-1 border-b px-3 py-1.5">
        {SUBS.map(({ id, label, icon: Icon }) => (
          <button
            key={id}
            type="button"
            aria-pressed={sub === id}
            onClick={() => choose(id)}
            className="flex h-7 items-center gap-1.5 rounded-md px-2.5 text-xs text-muted-foreground hover:bg-accent/50 hover:text-foreground focus-ring aria-pressed:bg-accent aria-pressed:text-foreground"
          >
            <Icon className="size-3.5" aria-hidden="true" />
            {label}
            {id === "next" && waiting > 0 && (
              <span className="rounded-full bg-red-500/90 px-1.5 text-3xs font-bold text-white tabular-nums" aria-label={`${waiting}건`}>
                {waiting}
              </span>
            )}
          </button>
        ))}
      </div>
      <div className="scrollbar-sleek min-h-0 flex-1 overflow-auto bg-chat-canvas p-4">
        {runs.length === 0 ? (
          <p className="text-sm text-muted-foreground">이 프로젝트에는 아직 작업이 없습니다.</p>
        ) : sub === "dashboard" ? (
          <WorkDashboard
            runs={runs}
            todo={todo}
            stopped={stopped}
            now={now}
            onOpen={openItem}
            onToggle={toggle}
            onNextUp={() => choose("next")}
            onBoard={() => choose("board")}
            onCard={(run, card) => onCard({ run, card })}
          />
        ) : sub === "next" ? (
          <NextUp todo={todo} stopped={stopped} now={now} handled={handled} onToggle={toggle} onOpen={openItem} />
        ) : (
          <AgentBoard runs={runs} onOpen={onCard} />
        )}
      </div>
    </div>
  );
}

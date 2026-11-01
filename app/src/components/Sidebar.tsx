import { CircleCheckBig, CircleDashed, SquarePen } from "lucide-react";

import type { CardState, RunView } from "../../../src/core/snapshot.ts";
import { PROVIDER_SHORT } from "../format.ts";
import type { ScreenState } from "../useRun.ts";
import { AgentStateDot } from "./AgentStateDot.tsx";

interface Props {
  state: ScreenState;
  runs: RunView[];
  selected?: string;
  card?: string;
  onRun: (runId: string) => void;
  onCard: (runId: string, cardId: string) => void;
  onCompose: () => void;
}

/** Orca's worktree sidebar, listing O runs instead of worktrees. */
export function Sidebar({ state, runs, selected, card, onRun, onCard, onCompose }: Props) {
  const shown = state === "full" ? runs : [];
  const live = shown.filter((r) => r.status === "running");
  const ended = shown.filter((r) => r.status !== "running");

  return (
    <aside className="flex min-h-0 flex-col border-r bg-sidebar text-sidebar-foreground" aria-label="작업 목록">
      <div className="flex h-10 shrink-0 items-center gap-2 px-4">
        <span className="text-sm font-semibold">Project O</span>
      </div>

      <nav className="flex flex-col gap-0.5 px-2 pb-2">
        <button
          type="button"
          onClick={onCompose}
          className="flex h-8 items-center gap-2.5 rounded-md px-2 text-[13px] text-sidebar-foreground/90 hover:bg-sidebar-accent focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
        >
          <SquarePen className="size-4 text-muted-foreground" aria-hidden="true" />새 요청
        </button>
      </nav>

      <div className="px-4 pt-2 pb-1">
        <span className="text-2xs font-semibold tracking-wider text-muted-foreground uppercase">작업</span>
      </div>

      <div className="scrollbar-sleek min-h-0 flex-1 overflow-y-auto px-2 pb-3" aria-busy={state === "loading"}>
        {state === "loading" && [0, 1].map((i) => <div key={i} className="mx-1 my-2 h-16 rounded-md bg-sidebar-accent/60" aria-hidden="true" />)}
        {state !== "loading" && shown.length === 0 && <p className="px-2 py-3 text-xs text-muted-foreground">아직 실행한 작업이 없습니다.</p>}
        {[
          { label: "진행 중", icon: <CircleDashed className="size-3.5 text-amber-500" />, list: live },
          { label: "끝남", icon: <CircleCheckBig className="size-3.5 text-emerald-500" />, list: ended },
        ]
          .filter((g) => g.list.length > 0)
          .map((g) => (
            <section key={g.label} className="mt-1">
              <h2 className="flex items-center gap-2 px-2 py-1.5 text-[13px] font-semibold">
                {g.icon}
                {g.label}
                <span className="rounded-full bg-muted px-1.5 text-3xs text-muted-foreground">{g.list.length}</span>
              </h2>
              {g.list.map((run) => (
                <RunRow key={run.runId} run={run} current={run.runId === selected} card={card} onRun={onRun} onCard={onCard} />
              ))}
            </section>
          ))}
      </div>
    </aside>
  );
}

const RUN_STATE: Record<RunView["status"], CardState> = { running: "running", done: "done", failed: "failed", stopped: "stopped" };

function RunRow({ run, current, card, onRun, onCard }: { run: RunView; current: boolean; card?: string; onRun: (id: string) => void; onCard: (runId: string, cardId: string) => void }) {
  return (
    <div data-current={current || undefined} className="mb-1 rounded-md px-2 py-2 hover:bg-sidebar-accent data-current:bg-sidebar-accent">
      <button
        type="button"
        onClick={() => onRun(run.runId)}
        aria-current={current || undefined}
        className="flex w-full items-center gap-2 text-left focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
      >
        <AgentStateDot state={RUN_STATE[run.status]} />
        <span className="truncate text-[13px]">{run.title}</span>
      </button>
      <div className="mt-1 flex items-center gap-1.5 pl-[18px] text-xs text-muted-foreground">
        <span className="rounded-sm bg-muted px-1 text-3xs font-medium text-foreground">{run.project.split("/").pop()}</span>
        <span className="truncate font-mono">{run.runId}</span>
      </div>
      <div className="mt-1.5 ml-[18px] flex flex-wrap items-center gap-1 rounded-md border px-1.5 py-1">
        {run.cards.map((c) => (
          <button
            key={c.id}
            type="button"
            onClick={() => onCard(run.runId, c.id)}
            aria-pressed={current && card === c.id}
            aria-label={`${c.label} 카드 열기`}
            className="flex items-center gap-1 rounded px-1 py-0.5 hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none aria-pressed:bg-accent"
          >
            <AgentStateDot state={c.state} />
            <span className="text-3xs font-semibold text-muted-foreground">{PROVIDER_SHORT[c.provider]}</span>
          </button>
        ))}
      </div>
    </div>
  );
}

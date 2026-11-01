import { CircleCheckBig, CircleDashed, MessageSquare, SquarePen } from "lucide-react";

import type { RunView } from "../../../src/core/snapshot.ts";
import { PROVIDER_SHORT } from "../format.ts";
import type { ScreenState } from "../useRun.ts";
import { AgentStateDot } from "./AgentStateDot.tsx";

interface Props {
  state: ScreenState;
  run?: RunView;
  tab: string;
  onTab: (tab: string) => void;
  onCompose: () => void;
}

/** Orca's worktree sidebar, listing O runs instead of worktrees. */
export function Sidebar({ state, run, tab, onTab, onCompose }: Props) {
  const runs = state === "full" && run ? [run] : [];
  const live = runs.filter((r) => r.status === "running");
  const ended = runs.filter((r) => r.status !== "running");

  return (
    <aside className="flex min-h-0 flex-col border-r bg-sidebar text-sidebar-foreground" aria-label="작업 목록">
      <div className="flex h-10 shrink-0 items-center gap-2 px-4">
        <span className="text-sm font-semibold">Project O</span>
        <span className="truncate text-xs text-muted-foreground">{run?.project.split("/").pop() ?? "todo-app"}</span>
      </div>

      <nav className="flex flex-col gap-0.5 px-2 pb-2">
        <NavItem icon={<SquarePen className="size-4" />} label="새 요청" onClick={onCompose} />
        <NavItem icon={<MessageSquare className="size-4" />} label="총괄 대화" current={tab === "lead"} onClick={() => onTab("lead")} />
      </nav>

      <div className="flex items-center justify-between px-4 pt-2 pb-1">
        <span className="text-2xs font-semibold tracking-wider text-muted-foreground uppercase">작업</span>
      </div>

      <div className="scrollbar-sleek min-h-0 flex-1 overflow-y-auto px-2 pb-3" aria-busy={state === "loading"}>
        {state === "loading" && [0, 1].map((i) => <div key={i} className="mx-1 my-2 h-16 rounded-md bg-sidebar-accent/60" aria-hidden="true" />)}
        {state !== "loading" && runs.length === 0 && <p className="px-2 py-3 text-xs text-muted-foreground">아직 실행한 작업이 없습니다.</p>}
        {live.length > 0 && <Group icon={<CircleDashed className="size-3.5 text-amber-500" />} label="진행 중" count={live.length} runs={live} tab={tab} onTab={onTab} />}
        {ended.length > 0 && <Group icon={<CircleCheckBig className="size-3.5 text-emerald-500" />} label="끝남" count={ended.length} runs={ended} tab={tab} onTab={onTab} />}
      </div>
    </aside>
  );
}

function NavItem({ icon, label, current, onClick }: { icon: React.ReactNode; label: string; current?: boolean; onClick: () => void }) {
  return (
    <button
      type="button"
      onClick={onClick}
      data-current={current || undefined}
      className="flex h-8 items-center gap-2.5 rounded-md px-2 text-[13px] text-sidebar-foreground/90 hover:bg-sidebar-accent focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none data-current:bg-sidebar-accent"
    >
      <span className="text-muted-foreground">{icon}</span>
      {label}
    </button>
  );
}

function Group({ icon, label, count, runs, tab, onTab }: { icon: React.ReactNode; label: string; count: number; runs: RunView[]; tab: string; onTab: (tab: string) => void }) {
  return (
    <section className="mt-1">
      <h2 className="flex items-center gap-2 px-2 py-1.5 text-[13px] font-semibold">
        {icon}
        {label}
        <span className="rounded-full bg-muted px-1.5 text-3xs text-muted-foreground">{count}</span>
      </h2>
      {runs.map((run) => (
        <RunRow key={run.runId} run={run} tab={tab} onTab={onTab} />
      ))}
    </section>
  );
}

function RunRow({ run, tab, onTab }: { run: RunView; tab: string; onTab: (tab: string) => void }) {
  const current = tab === "lead" || run.cards.some((c) => c.id === tab);
  const runState = run.status === "running" ? "running" : run.status === "done" ? "done" : run.status === "stopped" ? "stopped" : "failed";
  return (
    <div data-current={current || undefined} className="mb-1 rounded-md px-2 py-2 hover:bg-sidebar-accent data-current:bg-sidebar-accent">
      <button type="button" onClick={() => onTab("lead")} className="flex w-full items-center gap-2 text-left focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none">
        <AgentStateDot state={runState} />
        <span className="truncate text-[13px]">{run.title}</span>
      </button>
      <div className="mt-1 flex items-center gap-1.5 pl-[18px] text-xs text-muted-foreground">
        <span className="rounded-sm bg-muted px-1 text-3xs font-medium text-foreground">{run.project.split("/").pop()}</span>
        <span className="truncate font-mono">{run.runId}</span>
      </div>
      <div className="mt-1.5 ml-[18px] flex flex-wrap items-center gap-1 rounded-md border px-1.5 py-1">
        {run.cards.map((card) => (
          <button
            key={card.id}
            type="button"
            onClick={() => onTab(card.id)}
            aria-pressed={tab === card.id}
            aria-label={`${card.role} 카드 열기`}
            className="flex items-center gap-1 rounded px-1 py-0.5 hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none aria-pressed:bg-accent"
          >
            <AgentStateDot state={card.state} />
            <span className="text-3xs font-semibold text-muted-foreground">{PROVIDER_SHORT[card.provider]}</span>
          </button>
        ))}
      </div>
    </div>
  );
}

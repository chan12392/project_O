import { ExternalLink, FolderOpen } from "lucide-react";
import { useId, useMemo, useState, type ReactNode } from "react";

import { isSessionRun, nextUpByProject, projectName, recommend, type NextItem, type ProjectNext, type Suggestion } from "../../../src/core/attention.ts";
import type { RunView } from "../../../src/core/snapshot.ts";
import { clip } from "../format.ts";
import { NextTicket, STAMP, useHandled } from "./NextUp.tsx";
import { Stamp, TICKET_GHOST, TICKET_PRIMARY, Ticket } from "./Ticket.tsx";

interface Props {
  runs: RunView[];
  now: Date;
  loading?: boolean;
  /** Opens the ticket's card (or run) beside its project's lead. */
  onOpen: (item: NextItem) => void;
  onProject: (path: string) => void;
}

/**
 * Next up for every project at once: a few recommendations with the reason
 * each was picked, then each project's tickets, the most urgent project first.
 * O runs and Claude Code sessions are read the same way.
 */
export function AllNextUp({ runs, now, loading, onOpen, onProject }: Props) {
  const { handled, toggle } = useHandled();
  const groups = useMemo(() => nextUpByProject(runs, now, handled), [runs, now, handled]);
  const picks = useMemo(() => recommend(runs, now, handled), [runs, now, handled]);
  // Ticked this visit: still shown (struck through) so a slip can be undone.
  const [ticked, setTicked] = useState<NextItem[]>([]);
  const tick = (item: NextItem) => {
    setTicked((t) => (t.some((x) => x.key === item.key) ? t.filter((x) => x.key !== item.key) : [...t, item]));
    toggle(item.key);
  };
  const total = groups.reduce((sum, g) => sum + g.todo.length + g.stopped.length, 0);
  const projects = new Set(runs.map((r) => r.project)).size;
  const sessions = runs.filter(isSessionRun).length;
  const projectOf = new Map(runs.map((r) => [r.runId, r.project]));
  const struck = (project: string) => ticked.filter((t) => handled.has(t.key) && projectOf.get(t.runId) === project);
  // A project whose last ticket was just ticked keeps its place until the view is left.
  const shown: ProjectNext[] = [...groups, ...[...new Set(ticked.map((t) => projectOf.get(t.runId) ?? ""))].filter((p) => !groups.some((g) => g.project === p) && struck(p).length > 0).map((project) => ({ project, todo: [], stopped: [] }))];

  return (
    <section className="flex min-h-0 min-w-0 flex-1 flex-col" aria-labelledby="all-next-title">
      <header className="flex h-9 shrink-0 items-center gap-2 border-b bg-tab-bar px-4 text-xs">
        <h1 id="all-next-title" className="shrink-0 font-medium text-tab-active-foreground">
          다음 할 일
        </h1>
        <span className="truncate text-muted-foreground">
          프로젝트 {projects}곳{sessions > 0 && ` · Claude Code 세션 ${sessions}개 포함`}
        </span>
      </header>
      <div className="scrollbar-sleek min-h-0 flex-1 overflow-y-auto bg-chat-canvas" aria-busy={loading}>
        <div className="mx-auto flex w-full max-w-5xl flex-col gap-8 px-5 pt-5 pb-12">
          {loading ? (
            [0, 1, 2].map((i) => <div key={i} className="shimmer h-24 rounded-lg" aria-hidden="true" />)
          ) : (
            <>
              <section aria-labelledby="picks-title">
                <GroupHead id="picks-title" title="추천" count={picks.length} meta="멈춘 작업부터, 오래된 순 · 다음은 답할 말, 검토할 PR, 오래 쉰 프로젝트" />
                {picks.length === 0 ? (
                  <p className="rounded-lg border border-dashed px-4 py-5 text-center text-xs text-muted-foreground">
                    추천할 일이 없습니다. 멈춘 작업, 에이전트의 질문, 검토할 PR, 며칠째 손대지 않은 프로젝트가 생기면 이유와 함께 여기에 올립니다.
                  </p>
                ) : (
                  <div className="@container">
                    <ol className="grid gap-2.5 @2xl:grid-cols-2">
                      {picks.map((pick) => (
                        <li key={pick.key} className="min-w-0">
                          <PickTicket pick={pick} now={now} onOpen={onOpen} onProject={onProject} onSkip={() => (pick.item ? tick(pick.item) : toggle(pick.key))} />
                        </li>
                      ))}
                    </ol>
                  </div>
                )}
              </section>

              {total === 0 && shown.length === 0 ? (
                <p className="text-xs text-muted-foreground">모든 프로젝트에 지금 할 일이 없습니다.</p>
              ) : (
                shown.map((group) => (
                  <ProjectGroup key={group.project} group={group} struck={struck(group.project)} now={now} onOpen={onOpen} onProject={onProject} onTick={tick} />
                ))
              )}
            </>
          )}
        </div>
      </div>
    </section>
  );
}

function GroupHead({ id, title, count, meta, action }: { id: string; title: string; count: number; meta: string; action?: ReactNode }) {
  return (
    <header className="mb-2.5 flex min-w-0 items-baseline gap-2 border-b-2 border-muted-foreground/40 pb-1.5">
      <h2 id={id} className="shrink-0 text-sm font-bold">
        {title}
      </h2>
      <span className="shrink-0 text-xs font-semibold text-muted-foreground tabular-nums">{count}</span>
      <span className="min-w-0 truncate text-2xs text-muted-foreground" title={meta}>
        {meta}
      </span>
      {action && <span className="ml-auto shrink-0 self-center">{action}</span>}
    </header>
  );
}

const IDLE_STAMP = { label: "쉬는 프로젝트", tone: "heat" as const };

/** One recommendation: what it is, why it was picked, and the way in. */
function PickTicket({ pick, now, onOpen, onProject, onSkip }: { pick: Suggestion; now: Date; onOpen: (item: NextItem) => void; onProject: (path: string) => void; onSkip: () => void }) {
  const stamp = pick.kind === "idle" ? IDLE_STAMP : STAMP[pick.kind];
  const item = pick.item;
  return (
    <Ticket since={pick.since} now={now} code={projectName(pick.project)} stamp={<Stamp tone={stamp.tone}>{stamp.label}</Stamp>}>
      <h3 className="min-w-0 text-[13px] leading-snug font-semibold [overflow-wrap:anywhere]">{pick.title}</h3>
      <p className="text-xs font-semibold text-[var(--ink)]">{pick.reason}</p>
      {item?.detail && <p className="text-xs leading-relaxed text-[var(--ink-dim)] [overflow-wrap:anywhere]">{clip(item.detail, 140)}</p>}
      <div className="mt-1 flex flex-wrap items-center gap-1">
        {item?.pr ? (
          <a href={item.pr} target="_blank" rel="noreferrer" className={TICKET_PRIMARY}>
            <ExternalLink className="size-3.5" aria-hidden="true" />
            PR 열기
          </a>
        ) : item ? (
          <button type="button" onClick={() => onOpen(item)} className={TICKET_PRIMARY}>
            {item.kind === "answer" ? "대화 보기" : "기록 열기"}
          </button>
        ) : (
          <button type="button" onClick={() => onProject(pick.project)} className={TICKET_PRIMARY}>
            <FolderOpen className="size-3.5" aria-hidden="true" />
            프로젝트 열기
          </button>
        )}
        <button type="button" onClick={onSkip} className={TICKET_GHOST} aria-label={`${pick.title} 처리함으로 표시`}>
          처리함
        </button>
      </div>
    </Ticket>
  );
}

/** One project's tickets: what to do next, then what stopped. */
function ProjectGroup({ group, struck, now, onOpen, onProject, onTick }: { group: ProjectNext; struck: NextItem[]; now: Date; onOpen: (item: NextItem) => void; onProject: (path: string) => void; onTick: (item: NextItem) => void }) {
  const id = useId();
  const name = projectName(group.project);
  const items = [...group.stopped, ...group.todo];
  return (
    <section aria-labelledby={id}>
      <GroupHead
        id={id}
        title={name}
        count={items.length}
        meta={group.project}
        action={
          <button type="button" onClick={() => onProject(group.project)} className="flex h-6 items-center gap-1 rounded-md px-1.5 text-2xs text-muted-foreground hover:bg-accent hover:text-foreground focus-ring">
            <FolderOpen className="size-3.5" aria-hidden="true" />
            {name} 열기
          </button>
        }
      />
      <div className="@container">
        <div className="grid gap-2.5 @2xl:grid-cols-2">
          {items.map((item) => (
            <NextTicket key={item.key} item={item} now={now} onOpen={onOpen} onDone={() => onTick(item)} />
          ))}
          {struck.map((item) => (
            <NextTicket key={item.key} item={item} now={now} onOpen={onOpen} onDone={() => onTick(item)} done />
          ))}
        </div>
      </div>
    </section>
  );
}

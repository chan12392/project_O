import { ChevronRight, Coffee, Folder, FolderPlus, MoreHorizontal, Pencil, SquarePen, X } from "lucide-react";
import { useCallback, useId, useRef, useState } from "react";

import type { RunView } from "../../../src/core/snapshot.ts";
import { useDismiss } from "../dismiss.ts";
import { PROVIDER_SHORT, timeOf } from "../format.ts";
import { LEAD_WORD, type LeadState, type Project, type useProjectGroups } from "../projects.ts";
import type { ScreenState } from "../useRun.ts";
import { AgentStateDot } from "./AgentStateDot.tsx";

type Groups = ReturnType<typeof useProjectGroups>;

interface Props {
  state: ScreenState;
  projects: Project[];
  groups: Groups;
  leadState: (path: string) => LeadState;
  project?: string;
  run?: string;
  card?: string;
  onProject: (path: string) => void;
  onWake: (path: string) => void;
  onRun: (path: string, runId: string) => void;
  onCard: (runId: string, cardId: string) => void;
  onCompose: () => void;
}

/** Orca's repo sidebar: projects, optionally in groups, each with its lead and its runs. */
export function Sidebar({ state, projects, groups, leadState, project, run, card, onProject, onWake, onRun, onCard, onCompose }: Props) {
  const shown = state === "full" ? projects : [];
  const [naming, setNaming] = useState<{ project?: string } | undefined>();
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set(project ? [project] : []));
  const toggle = (path: string) =>
    setExpanded((s) => {
      const next = new Set(s);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  const select = (path: string) => {
    setExpanded((s) => new Set(s).add(path));
    onProject(path);
  };

  const row = (p: Project) => (
    <ProjectRow
      key={p.path}
      project={p}
      groups={groups}
      lead={leadState(p.path)}
      current={p.path === project}
      open={expanded.has(p.path) || p.path === project}
      run={run}
      card={card}
      onSelect={() => select(p.path)}
      onToggle={() => toggle(p.path)}
      onWake={() => onWake(p.path)}
      onRun={(runId) => onRun(p.path, runId)}
      onCard={onCard}
      onNewGroup={() => setNaming({ project: p.path })}
    />
  );
  const loose = shown.filter((p) => !groups.groups.some((g) => g.id === groups.member[p.path]));

  return (
    <aside className="flex min-h-0 flex-col border-r bg-sidebar text-sidebar-foreground" aria-label="프로젝트 목록">
      <div className="flex h-10 shrink-0 items-center gap-2 px-4">
        <span className="text-sm font-semibold">Project O</span>
      </div>

      <nav className="flex flex-col gap-0.5 px-2 pb-2">
        <button type="button" onClick={onCompose} className="flex h-8 items-center gap-2.5 rounded-md px-2 text-[13px] text-sidebar-foreground/90 hover:bg-sidebar-accent focus-ring">
          <SquarePen className="size-4 text-muted-foreground" aria-hidden="true" />새 요청
        </button>
      </nav>

      <div className="flex items-center px-4 pt-2 pb-1">
        <span className="text-2xs font-semibold tracking-wider text-muted-foreground uppercase">프로젝트</span>
        <button
          type="button"
          onClick={() => setNaming({})}
          className="ml-auto flex size-6 items-center justify-center rounded-md text-muted-foreground hover:bg-sidebar-accent hover:text-foreground focus-ring"
          aria-label="그룹 만들기"
          title="그룹 만들기"
        >
          <FolderPlus className="size-3.5" aria-hidden="true" />
        </button>
      </div>

      {naming && (
        <GroupNameInput
          label={naming.project ? `${projects.find((p) => p.path === naming.project)?.name}을(를) 넣을 새 그룹` : "새 그룹"}
          onDone={(name) => {
            if (name) groups.addGroup(name, naming.project);
            setNaming(undefined);
          }}
        />
      )}

      <div className="scrollbar-sleek min-h-0 flex-1 overflow-y-auto px-2 pb-3" aria-busy={state === "loading"}>
        {state === "loading" && [0, 1].map((i) => <div key={i} className="shimmer mx-1 my-2 h-10 rounded-md" aria-hidden="true" />)}
        {state !== "loading" && shown.length === 0 && <p className="px-2 py-3 text-xs text-muted-foreground">아직 작업한 프로젝트가 없습니다.</p>}

        {groups.groups.map((g) => {
          const members = shown.filter((p) => groups.member[p.path] === g.id);
          return (
            <section key={g.id} className="mt-1" aria-label={`${g.name} 그룹`}>
              <GroupHead group={g} count={members.length} groups={groups} />
              {!g.collapsed && (
                <div className="ml-2 border-l pl-1">
                  {members.length === 0 && <p className="px-2 py-1.5 text-2xs text-muted-foreground">프로젝트의 ⋯ 메뉴에서 이 그룹으로 옮길 수 있습니다.</p>}
                  {members.map(row)}
                </div>
              )}
            </section>
          );
        })}
        {groups.groups.length > 0 && loose.length > 0 && <h2 className="mt-2 px-2 py-1.5 text-2xs font-semibold text-muted-foreground">그룹 없음</h2>}
        {loose.map(row)}
      </div>
    </aside>
  );
}

type Group = Groups["groups"][number];

/** A group's header: fold, rename (pencil or double-click, Enter saves, Esc cancels) and ungroup. */
function GroupHead({ group: g, count, groups }: { group: Group; count: number; groups: Groups }) {
  const [renaming, setRenaming] = useState(false);
  const head = useRef<HTMLButtonElement>(null);
  const finish = (name: string | undefined) => {
    if (name && name !== g.name) groups.rename(g.id, name);
    setRenaming(false);
    // Enter or Esc leaves focus nowhere; put it back on the header. A click elsewhere keeps its target.
    requestAnimationFrame(() => {
      if (document.activeElement === document.body) head.current?.focus();
    });
  };
  if (renaming) return <GroupNameInput label={`${g.name} 그룹의 새`} initial={g.name} onDone={finish} />;
  const icon = "flex size-6 shrink-0 items-center justify-center rounded-md text-muted-foreground opacity-0 group-hover/head:opacity-100 hover:text-foreground focus-visible:opacity-100 focus-ring";
  return (
    <div className="group/head flex items-center rounded-md hover:bg-sidebar-accent/60">
      <button
        ref={head}
        type="button"
        onClick={() => groups.toggle(g.id)}
        onDoubleClick={() => setRenaming(true)}
        aria-expanded={!g.collapsed}
        className="flex min-w-0 flex-1 items-center gap-1.5 px-2 py-1.5 text-[13px] font-semibold focus-ring"
      >
        <ChevronRight className={`size-3.5 shrink-0 text-muted-foreground transition-transform ${g.collapsed ? "" : "rotate-90"}`} aria-hidden="true" />
        <span className="truncate">{g.name}</span>
        <span className="rounded-full bg-muted px-1.5 text-3xs font-normal text-muted-foreground">{count}</span>
      </button>
      <button type="button" onClick={() => setRenaming(true)} className={icon} aria-label={`${g.name} 그룹 이름 바꾸기`} title="이름 바꾸기 (두 번 눌러도 됨)">
        <Pencil className="size-3" aria-hidden="true" />
      </button>
      <button type="button" onClick={() => groups.remove(g.id)} className={`mr-1 ${icon}`} aria-label={`${g.name} 그룹 풀기 (프로젝트는 남음)`} title="그룹 풀기 (프로젝트는 남음)">
        <X className="size-3.5" aria-hidden="true" />
      </button>
    </div>
  );
}

function GroupNameInput({ label, initial, onDone: done }: { label: string; initial?: string; onDone: (name: string | undefined) => void }) {
  const id = useId();
  // Enter, Escape and the blur that follows them must finish only once.
  const finished = useRef(false);
  const onDone = (name: string | undefined) => {
    if (finished.current) return;
    finished.current = true;
    done(name);
  };
  return (
    <form
      className="px-3 pb-2"
      onSubmit={(e) => {
        e.preventDefault();
        onDone(new FormData(e.currentTarget).get("name")?.toString().trim() || undefined);
      }}
    >
      <label className="mb-1 block text-2xs text-muted-foreground" htmlFor={id}>
        {label} 이름 (Enter 저장, Esc 취소)
      </label>
      <input
        id={id}
        name="name"
        defaultValue={initial}
        autoFocus
        onFocus={(e) => e.currentTarget.select()}
        maxLength={30}
        onKeyDown={(e) => e.key === "Escape" && onDone(undefined)}
        onBlur={(e) => onDone(e.currentTarget.value.trim() || undefined)}
        className="h-7 w-full rounded-md border bg-background px-2 text-xs focus-ring"
      />
    </form>
  );
}

interface RowProps {
  project: Project;
  groups: Groups;
  lead: LeadState;
  current: boolean;
  open: boolean;
  run?: string;
  card?: string;
  onSelect: () => void;
  onToggle: () => void;
  onWake: () => void;
  onRun: (runId: string) => void;
  onCard: (runId: string, cardId: string) => void;
  onNewGroup: () => void;
}

function ProjectRow({ project, groups, lead, current, open, run, card, onSelect, onToggle, onWake, onRun, onCard, onNewGroup }: RowProps) {
  const live = project.runs.filter((r) => r.status === "running").length;
  return (
    <div className="mt-0.5">
      <div data-current={current || undefined} className="group/row flex items-center rounded-md hover:bg-sidebar-accent data-current:bg-sidebar-accent">
        <button type="button" onClick={onToggle} aria-expanded={open} aria-label={`${project.name} 작업 목록 ${open ? "접기" : "펼치기"}`} className="flex size-7 shrink-0 items-center justify-center rounded-md text-muted-foreground focus-ring">
          <ChevronRight className={`size-3.5 transition-transform ${open ? "rotate-90" : ""}`} aria-hidden="true" />
        </button>
        <button type="button" onClick={onSelect} aria-current={current || undefined} className="flex min-w-0 flex-1 items-center gap-2 py-1.5 pr-1 text-left focus-ring">
          <Folder className="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
          <span className="truncate text-[13px] font-medium">{project.name}</span>
          {live > 0 && <span className="shrink-0 rounded-full bg-agent-working/15 px-1.5 text-3xs text-agent-working">{live} 진행</span>}
        </button>
        <ProjectMenu project={project} groups={groups} onNewGroup={onNewGroup} />
      </div>
      {open && (
        <div className="ml-3.5 border-l pb-1 pl-1.5">
          <button
            type="button"
            onClick={lead === "resting" ? onWake : onSelect}
            className="flex w-full items-center gap-2 rounded-md px-2 py-1 text-left text-xs hover:bg-sidebar-accent focus-ring"
            aria-label={`총괄, ${LEAD_WORD[lead]}${lead === "resting" ? ". 눌러서 최근 세션 불러오기" : ""}`}
          >
            {lead === "resting" ? <Coffee className="size-3.5 text-muted-foreground" aria-hidden="true" /> : <AgentStateDot state={lead === "working" ? "running" : lead === "loading" ? "queued" : "done"} />}
            <span className="font-medium">총괄</span>
            <span className="text-muted-foreground">{LEAD_WORD[lead]}</span>
          </button>
          {project.runs.map((r) => (
            <RunRow key={r.runId} run={r} current={r.runId === run} card={card} onRun={onRun} onCard={onCard} />
          ))}
        </div>
      )}
    </div>
  );
}

/** Pattern from beakho-design lucian-074 (header dropdown): a small menu panel under its button. */
function ProjectMenu({ project, groups, onNewGroup }: { project: Project; groups: Groups; onNewGroup: () => void }) {
  const [open, setOpen] = useState(false);
  const panel = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  const close = useCallback(() => setOpen(false), []);
  useDismiss(panel, open, close, button);
  const inGroup = groups.member[project.path];
  const item = "flex w-full items-center rounded px-2 py-1.5 text-left text-xs hover:bg-accent focus-ring";
  return (
    <div className="relative shrink-0">
      <button
        ref={button}
        type="button"
        onClick={() => setOpen((o) => !o)}
        aria-expanded={open}
        aria-label={`${project.name} 메뉴`}
        className="mr-1 flex size-6 items-center justify-center rounded-md text-muted-foreground opacity-0 group-hover/row:opacity-100 hover:text-foreground focus-visible:opacity-100 aria-expanded:opacity-100 focus-ring"
      >
        <MoreHorizontal className="size-3.5" aria-hidden="true" />
      </button>
      {open && (
        <div ref={panel} role="group" aria-label={`${project.name} 그룹`} className="bloom absolute top-full right-0 z-20 mt-1 w-48 rounded-lg border bg-popover p-1 text-popover-foreground shadow-lg">
          <p className="px-2 pt-1 pb-1.5 text-3xs font-semibold text-muted-foreground">그룹으로 옮기기</p>
          {groups.groups
            .filter((g) => g.id !== inGroup)
            .map((g) => (
              <button key={g.id} type="button" className={item} onClick={() => (groups.move(project.path, g.id), close())}>
                {g.name}
              </button>
            ))}
          <button type="button" className={item} onClick={() => (onNewGroup(), close())}>
            새 그룹 만들어 넣기…
          </button>
          {inGroup && (
            <button type="button" className={`${item} text-muted-foreground`} onClick={() => (groups.move(project.path, undefined), close())}>
              그룹에서 빼기
            </button>
          )}
        </div>
      )}
    </div>
  );
}

function RunRow({ run, current, card, onRun, onCard }: { run: RunView; current: boolean; card?: string; onRun: (id: string) => void; onCard: (runId: string, cardId: string) => void }) {
  return (
    <div data-current={current || undefined} className="rounded-md px-2 py-1.5 hover:bg-sidebar-accent data-current:bg-sidebar-accent/70">
      <button type="button" onClick={() => onRun(run.runId)} aria-current={current || undefined} className="flex w-full items-center gap-2 text-left focus-ring">
        <AgentStateDot state={run.status} />
        <span className="truncate text-xs">{run.title}</span>
        <span className="ml-auto shrink-0 text-3xs text-muted-foreground tabular-nums">{timeOf(run.startedAt)}</span>
      </button>
      <div className="mt-1 ml-[18px] flex flex-wrap items-center gap-0.5">
        {run.cards.map((c) => (
          <button
            key={c.id}
            type="button"
            onClick={() => onCard(run.runId, c.id)}
            aria-pressed={current && card === c.id}
            aria-label={`${c.label} 카드 열기`}
            title={c.label}
            className="flex items-center gap-1 rounded px-1 py-0.5 hover:bg-accent aria-pressed:bg-accent focus-ring"
          >
            <AgentStateDot state={c.state} />
            <span className="text-3xs font-semibold text-muted-foreground">{PROVIDER_SHORT[c.provider]}</span>
          </button>
        ))}
      </div>
    </div>
  );
}

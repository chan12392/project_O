import { BookOpen, CalendarClock, ChevronRight, Coffee, Folder, FolderPlus, FolderX, LayoutDashboard, ListTodo, MoreHorizontal, Pencil, Plus, Search, SquarePen, Waypoints, X } from "lucide-react";
import { useCallback, useEffect, useId, useRef, useState, type ReactNode } from "react";

import { isSessionRun } from "../../../src/core/attention.ts";
import { chipSummary, isRecent, sameFolder, type AttentionKind } from "../../../src/core/projects.ts";
import type { RunView } from "../../../src/core/snapshot.ts";
import { useDismiss } from "../lib/dismiss.ts";
import { PROVIDER_SHORT, STATE_WORD, timeOf } from "../lib/format.ts";
import { isDirecting, LEAD_WORD, useTestGroupOpen, type LeadState, type Project, type useProjectGroups } from "../data/projects.ts";
import type { ScreenState } from "../data/useRun.ts";
import { CardSummary, summaryWords } from "../features/office/CardSummary.tsx";
import { AddFolderForm, canPickFolder, NewProjectForm, pickAndRegister } from "../features/projects/ProjectForms.tsx";
import { AgentStateDot } from "../ui/AgentStateDot.tsx";
import { backend } from "../platform/backend/index.ts";

type Groups = ReturnType<typeof useProjectGroups>;

/** Runs shown under a project before the rest fold into "이전 실행 N개". */
const SHOWN_RUNS = 3;
/** The hover/focus card list stops here. */
const TIP_CARDS = 12;

interface Props {
  state: ScreenState;
  projects: Project[];
  groups: Groups;
  leadState: (path: string) => LeadState;
  project?: string;
  run?: string;
  card?: string;
  now: Date;
  /** Why the project list could not be read; the sidebar then shows every folder in history. */
  listError?: string;
  /** The projects root, for the empty list's hint. */
  root?: string;
  onProject: (path: string) => void;
  onWake: (path: string) => void;
  onRun: (path: string, runId: string) => void;
  onCard: (runId: string, cardId: string) => void;
  /** "새 요청": choose (or make) the project whose lead gets it. */
  onNewRequest: () => void;
  onSearch: () => void;
  onDashboard: () => void;
  onAutomation: () => void;
  onWiki: () => void;
  onGraph: () => void;
  currentScreen?: string;
  /** A folder just put on the list (registered or made): open it. */
  onListed: (path: string) => void;
  onUnregister: (path: string) => void;
  /** Tickets waiting across every project, and whether that view is open. */
  nextCount?: number;
  nextOpen?: boolean;
  /** Whether the new-request screen is open. */
  newOpen?: boolean;
  onNextUp?: () => void;
  /** The account footer pinned under the project list. */
  footer?: ReactNode;
}

const HEAD_BUTTON = "flex size-6 items-center justify-center rounded-md text-muted-foreground hover:bg-sidebar-accent hover:text-foreground focus-ring aria-expanded:bg-sidebar-accent aria-expanded:text-foreground disabled:opacity-50";

/** Orca's repo sidebar: the listed projects (root, registered, test group), each with its lead and recent runs (ADR-0009). */
export function Sidebar(props: Props) {
  const { state, projects, groups, leadState, project, run, card, now, listError, root, onProject, onWake, onRun, onCard, onNewRequest, onSearch, onDashboard, onAutomation, onWiki, onGraph, currentScreen, onListed, onUnregister, nextCount = 0, nextOpen, onNextUp, footer } = props;
  const shown = state === "full" ? projects : [];
  const [naming, setNaming] = useState<{ project?: string } | undefined>();
  const [adding, setAdding] = useState<"new" | "folder" | undefined>();
  const [notice, setNotice] = useState<string>();
  const [picking, setPicking] = useState(false);
  const [testOpen, toggleTest] = useTestGroupOpen();
  const listedPath = (path: string | undefined) => (path === undefined ? undefined : shown.find((p) => sameFolder(p.path, path))?.path);
  const [expanded, setExpanded] = useState<Set<string>>(() => new Set(listedPath(project) ? [listedPath(project)!] : []));
  // The project on screen opens once when it becomes current; after that its chevron folds it like any other.
  const opened = useRef(listedPath(project) ? project : undefined);
  useEffect(() => {
    const path = listedPath(project);
    if (!path || opened.current === project) return;
    opened.current = project;
    setExpanded((s) => (s.has(path) ? s : new Set(s).add(path)));
  });
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
  const listed = (path: string) => {
    setAdding(undefined);
    setNotice(undefined);
    setExpanded((s) => new Set(s).add(path));
    onListed(path);
  };
  // The desktop app opens the native picker; the browser preview asks for the path.
  const addFolder = async () => {
    setNotice(undefined);
    if (!canPickFolder()) return setAdding((a) => (a === "folder" ? undefined : "folder"));
    setPicking(true);
    try {
      const path = await pickAndRegister();
      if (path) listed(path);
    } catch (e) {
      setNotice(e instanceof Error ? e.message : String(e));
    } finally {
      setPicking(false);
    }
  };

  const isCurrent = (p: Project) => project !== undefined && sameFolder(p.path, project);
  const row = (p: Project) => (
    <ProjectRow
      key={p.path}
      project={p}
      groups={groups}
      lead={leadState(p.path)}
      current={isCurrent(p)}
      open={expanded.has(p.path)}
      run={run}
      card={card}
      now={now}
      onSelect={() => select(p.path)}
      onToggle={() => toggle(p.path)}
      onWake={() => onWake(p.path)}
      onRun={(runId) => onRun(p.path, runId)}
      onCard={onCard}
      onNewGroup={() => setNaming({ project: p.path })}
      onUnregister={() => onUnregister(p.path)}
    />
  );
  const tests = shown.filter((p) => p.kind === "test");
  const others = shown.filter((p) => p.kind !== "test");
  const loose = others.filter((p) => !groups.groups.some((g) => g.id === groups.member[p.path]));
  const testShown = testOpen || tests.some(isCurrent);

  return (
    <aside className="flex min-h-0 flex-col border-r bg-sidebar text-sidebar-foreground" aria-label="프로젝트 목록">
      <nav className="flex flex-col gap-0.5 px-2 pt-3 pb-2">
        {([ ["wiki", "WIKI", BookOpen, onWiki], ["graph", "Graph", Waypoints, onGraph], ["search", "검색", Search, onSearch], ["dashboard", "대시보드", LayoutDashboard, onDashboard] ] as const).map(([id, label, Icon, action]) => <button key={id} type="button" onClick={action} aria-current={currentScreen === id ? "page" : undefined} className="flex h-8 items-center gap-2.5 rounded-md px-2 text-[13px] text-sidebar-foreground/90 hover:bg-sidebar-accent focus-ring aria-[current=page]:bg-sidebar-accent aria-[current=page]:font-medium"><Icon className="size-4 text-muted-foreground" aria-hidden="true" />{label}</button>)}
        {onNextUp && (
          <button
            type="button"
            onClick={onNextUp}
            aria-current={nextOpen ? "page" : undefined}
            className="flex h-8 items-center gap-2.5 rounded-md px-2 text-[13px] text-sidebar-foreground/90 hover:bg-sidebar-accent focus-ring aria-[current=page]:bg-sidebar-accent aria-[current=page]:font-medium"
          >
            <ListTodo className="size-4 text-muted-foreground" aria-hidden="true" />
            다음 할 일
            {nextCount > 0 && (
              <span className="ml-auto rounded-full bg-destructive px-1.5 text-3xs font-bold text-destructive-foreground tabular-nums">
                {nextCount}
                <span className="sr-only">건</span>
              </span>
            )}
          </button>
        )}
        <button type="button" onClick={onAutomation} aria-current={currentScreen === "automation" ? "page" : undefined} className="flex h-8 items-center gap-2.5 rounded-md px-2 text-[13px] text-sidebar-foreground/90 hover:bg-sidebar-accent focus-ring aria-[current=page]:bg-sidebar-accent aria-[current=page]:font-medium"><CalendarClock className="size-4 text-muted-foreground" aria-hidden="true" />자동화</button>
      </nav>

      <div className="flex items-center gap-0.5 px-4 pt-2 pb-1">
        <span className="text-2xs font-semibold tracking-wider text-muted-foreground uppercase">프로젝트</span>
        <button type="button" onClick={onNewRequest} aria-label="새 요청" title="새 요청" className={`ml-auto ${HEAD_BUTTON}`}><SquarePen className="size-3.5" aria-hidden="true" /></button>
        <button
          type="button"
          onClick={() => {
            setNotice(undefined);
            setAdding((a) => (a === "new" ? undefined : "new"));
          }}
          aria-expanded={adding === "new"}
          className={HEAD_BUTTON}
          aria-label="새 프로젝트 (루트에 빈 폴더 만들기)"
          title="새 프로젝트 (루트에 빈 폴더 만들기)"
        >
          <FolderPlus className="size-3.5" aria-hidden="true" />
        </button>
        <button
          type="button"
          onClick={() => void addFolder()}
          disabled={picking}
          aria-expanded={canPickFolder() ? undefined : adding === "folder"}
          className={HEAD_BUTTON}
          aria-label="프로젝트 추가 (폴더 등록)"
          title="프로젝트 추가 (폴더 등록)"
        >
          <Plus className="size-3.5" aria-hidden="true" />
        </button>
      </div>

      {adding === "new" && <NewProjectForm compact onDone={listed} onCancel={() => setAdding(undefined)} />}
      {adding === "folder" && <AddFolderForm compact onDone={listed} onCancel={() => setAdding(undefined)} />}
      {notice && (
        <p role="alert" className="mx-3 mb-2 flex items-start gap-1 rounded-md bg-destructive/10 px-2 py-1.5 text-2xs text-destructive">
          <span className="min-w-0 flex-1">{notice}</span>
          <button type="button" onClick={() => setNotice(undefined)} aria-label="알림 닫기" className="shrink-0 rounded focus-ring">
            <X className="size-3" aria-hidden="true" />
          </button>
        </p>
      )}
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
        {listError && state === "full" && (
          <p className="px-2 py-1.5 text-2xs text-muted-foreground" title={listError}>
            프로젝트 목록을 읽지 못해 기록에 있는 폴더를 모두 보여 줍니다.
          </p>
        )}
        {state !== "loading" && shown.length === 0 && (
          <p className="px-2 py-3 text-xs text-muted-foreground">
            목록에 프로젝트가 없습니다. 위의 + 로 폴더를 등록하거나 새 프로젝트를 만드세요.
            {root && (
              <span className="mt-1 block text-2xs">
                <span className="font-mono break-all">{root}</span> 안의 폴더는 자동으로 보입니다.
              </span>
            )}
          </p>
        )}

        {groups.groups.map((g) => {
          const members = others.filter((p) => groups.member[p.path] === g.id);
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

        {tests.length > 0 && (
          <section className="mt-2" aria-label="test 묶음">
            <div className="flex items-center rounded-md hover:bg-sidebar-accent/60">
              <button
                type="button"
                onClick={toggleTest}
                aria-expanded={testShown}
                title="프로젝트 루트의 test 폴더: 시험용 프로젝트"
                className="flex min-w-0 flex-1 items-center gap-1.5 px-2 py-1.5 text-[13px] font-semibold focus-ring"
              >
                <ChevronRight className={`size-3.5 shrink-0 text-muted-foreground transition-transform ${testShown ? "rotate-90" : ""}`} aria-hidden="true" />
                <span className="truncate">test</span>
                <span className="rounded-full bg-muted px-1.5 text-3xs font-normal text-muted-foreground">{tests.length}</span>
              </button>
            </div>
            {testShown && <div className="ml-2 border-l pl-1">{tests.map(row)}</div>}
          </section>
        )}
      </div>
      {footer}
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
  now: Date;
  onSelect: () => void;
  onToggle: () => void;
  onWake: () => void;
  onRun: (runId: string) => void;
  onCard: (runId: string, cardId: string) => void;
  onNewGroup: () => void;
  onUnregister: () => void;
}

function ProjectRow({ project, groups, lead, current, open, run, card, now, onSelect, onToggle, onWake, onRun, onCard, onNewGroup, onUnregister }: RowProps) {
  const [menuOpen, setMenuOpen] = useState(false);
  // Only O's own runs make a project busy: a Claude Code session open here is not one (B7).
  const live = project.runs.filter(isDirecting).length;
  const recent = project.runs.slice(0, SHOWN_RUNS);
  const older = project.runs.slice(SHOWN_RUNS);
  // The older runs open on their own only to show a selected run that is still recent.
  const chosenOlder = older.find((r) => r.runId === run);
  const [olderOpen, setOlderOpen] = useState(false);
  const olderShown = olderOpen || Boolean(chosenOlder && isRecent(chosenOlder, now));
  const olderLive = older.filter(isDirecting).length;
  const runRow = (r: RunView) => <RunRow key={r.runId} run={r} current={r.runId === run} card={card} now={now} onRun={onRun} onCard={onCard} />;

  return (
    <div className="mt-0.5">
      <div data-current={current || undefined} onContextMenu={(e) => { e.preventDefault(); setMenuOpen(true); }} className="group/row flex items-center rounded-md hover:bg-sidebar-accent data-current:bg-sidebar-accent">
        <button type="button" onClick={onToggle} aria-expanded={open} aria-label={`${project.name} 작업 목록 ${open ? "접기" : "펼치기"}`} className="flex size-7 shrink-0 items-center justify-center rounded-md text-muted-foreground focus-ring">
          <ChevronRight className={`size-3.5 transition-transform ${open ? "rotate-90" : ""}`} aria-hidden="true" />
        </button>
        <button type="button" onClick={onSelect} aria-current={current || undefined} title={project.path} className="flex min-w-0 flex-1 items-center gap-2 py-1.5 pr-1 text-left focus-ring">
          {project.missing ? <FolderX className="size-3.5 shrink-0 text-destructive" aria-hidden="true" /> : <Folder className="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />}
          <span className={`truncate text-[13px] font-medium ${project.missing ? "text-muted-foreground line-through" : ""}`}>{project.name}</span>
          {project.missing && <span className="sr-only">(폴더 없음)</span>}
          {live > 0 && <span className="shrink-0 rounded-full bg-agent-working/15 px-1.5 text-3xs text-agent-working">{live} 진행</span>}
        </button>
        <ProjectMenu project={project} groups={groups} onNewGroup={onNewGroup} onUnregister={onUnregister} open={menuOpen} setOpen={setMenuOpen} />
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
          {recent.map(runRow)}
          {older.length > 0 && (
            <>
              <button
                type="button"
                onClick={() => setOlderOpen(!olderShown)}
                aria-expanded={olderShown}
                data-current={(chosenOlder && !olderShown) || undefined}
                className="flex w-full items-center gap-1.5 rounded-md px-2 py-1 text-left text-2xs text-muted-foreground hover:bg-sidebar-accent hover:text-foreground focus-ring data-current:bg-sidebar-accent/70 data-current:text-foreground"
              >
                <ChevronRight className={`size-3 shrink-0 transition-transform ${olderShown ? "rotate-90" : ""}`} aria-hidden="true" />
                이전 실행 {older.length}개
                {olderLive > 0 && <span className="text-agent-working">· {olderLive} 진행</span>}
                {chosenOlder && !olderShown && <span className="sr-only">(선택한 실행이 여기 있음)</span>}
              </button>
              {olderShown && older.map(runRow)}
            </>
          )}
        </div>
      )}
    </div>
  );
}

/** A small menu panel under its button. */
function ProjectMenu({ project, groups, onNewGroup, onUnregister, open, setOpen }: { project: Project; groups: Groups; onNewGroup: () => void; onUnregister: () => void; open: boolean; setOpen: (open: boolean | ((open: boolean) => boolean)) => void }) {
  const [error, setError] = useState("");
  const panel = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  const close = useCallback(() => setOpen(false), []);
  useDismiss(panel, open, close, button);
  const inGroup = groups.member[project.path];
  const item = "flex w-full items-center rounded px-2 py-1.5 text-left text-xs hover:bg-accent focus-ring";
  const movable = project.kind !== "test";
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
        <div ref={panel} role="group" aria-label={`${project.name} 메뉴`} className="bloom absolute top-full right-0 z-20 mt-1 w-52 rounded-lg border bg-popover p-1 text-popover-foreground shadow-lg">
          <button type="button" className={item} disabled={project.missing} onClick={() => { void backend(new URLSearchParams(window.location.search).get("source") === "demo").projectOpen(project.path).then(close, (e: unknown) => setError(String(e))); }}>폴더 열기</button>
          <button type="button" className={item} onClick={() => { void navigator.clipboard.writeText(project.path).then(close, (e: unknown) => setError(String(e))); }}>경로 복사</button>
          {error && <p role="alert" className="px-2 py-1 text-2xs text-destructive">{error}</p>}
          {movable && (
            <>
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
            </>
          )}
          {project.kind === "registered" ? (
            <>
              <div className="my-1 border-t" role="separator" />
              <button type="button" className={item} onClick={() => (onUnregister(), close())}>
                목록에서 빼기
              </button>
              <p className="px-2 pb-1 text-3xs text-muted-foreground">폴더와 기록은 지우지 않습니다.</p>
            </>
          ) : (
            project.kind !== "history" && (
              <p className={`px-2 pb-1 text-3xs text-muted-foreground ${movable ? "mt-1 border-t pt-1.5" : "pt-1"}`}>
                프로젝트 루트{project.kind === "test" ? "의 test 폴더" : ""} 안의 폴더라 목록에서 빼려면 폴더를 루트 밖으로 옮기세요.
              </p>
            )
          )}
        </div>
      )}
    </div>
  );
}

const WHY_WORD: Record<AttentionKind, string> = { failed: "실패", answer: "답을 기다림", silent: "조용함", limit: "한도 걸림" };

/**
 * A run: its title and time, then one line summing its cards (each agent once with a count,
 * each state with a count). Cards needing a look stay as chips; the whole list shows on
 * hover or focus, and in the right panel once the run is selected.
 */
function RunRow({ run, current, card, now, onRun, onCard }: { run: RunView; current: boolean; card?: string; now: Date; onRun: (id: string) => void; onCard: (runId: string, cardId: string) => void }) {
  const tipId = useId();
  const summary = chipSummary(run, now);
  const chip = (c: RunView["cards"][number], why?: AttentionKind) => (
    <button
      key={c.id}
      type="button"
      onClick={() => onCard(run.runId, c.id)}
      aria-pressed={current && card === c.id}
      aria-label={`${c.label} 카드 열기${why ? `, ${WHY_WORD[why]}` : ""}`}
      title={why ? `${c.label} · ${WHY_WORD[why]}` : c.label}
      className="flex max-w-full min-w-0 items-center gap-1 rounded px-1 py-0.5 hover:bg-accent aria-pressed:bg-accent focus-ring"
    >
      <AgentStateDot state={c.state} />
      <span className="shrink-0 text-3xs font-semibold text-muted-foreground">{PROVIDER_SHORT[c.provider]}</span>
      <span className={`truncate text-3xs ${why === "failed" ? "text-destructive" : why ? "text-agent-question-text" : "text-foreground"}`}>{c.label}</span>
    </button>
  );
  // Chips: the cards needing a look, and the card open in the right panel (its highlight stays visible).
  const chips: { card: RunView["cards"][number]; why?: AttentionKind }[] = [...summary.attention];
  const open = current && card ? run.cards.find((c) => c.id === card) : undefined;
  if (open && !chips.some((c) => c.card.id === open.id)) chips.push({ card: open });
  // When every card is a chip already, the summary line would only repeat them.
  const summed = chips.length < run.cards.length;

  return (
    <div data-current={current || undefined} className="group/run relative rounded-md px-2 py-1.5 hover:bg-sidebar-accent data-current:bg-sidebar-accent/70">
      <button
        type="button"
        onClick={() => onRun(run.runId)}
        aria-current={current || undefined}
        aria-describedby={run.cards.length > 0 ? tipId : undefined}
        className="flex w-full items-center gap-2 text-left focus-ring"
      >
        <AgentStateDot state={run.status} />
        <span className="truncate text-xs">{run.title}</span>
        {isSessionRun(run) && (
          <span className="shrink-0 rounded-sm bg-muted px-1 text-3xs text-muted-foreground" title="Claude Code 세션 (읽기 전용)">
            세션
          </span>
        )}
        <span className="ml-auto shrink-0 text-3xs text-muted-foreground tabular-nums">{timeOf(run.startedAt)}</span>
      </button>
      {summed && <CardSummary summary={summary} className="mt-1 ml-[18px]" />}
      {chips.length > 0 && <div className="mt-0.5 ml-[18px] flex flex-col items-start gap-0.5">{chips.map((c) => chip(c.card, c.why))}</div>}
      {run.cards.length > 0 && (
        <div
          id={tipId}
          role="tooltip"
          className="pointer-events-none invisible absolute top-full right-1 left-6 z-30 mt-0.5 rounded-md border bg-popover px-2 py-1.5 text-popover-foreground opacity-0 shadow-lg transition-[opacity,visibility] duration-150 group-hover/run:visible group-hover/run:delay-300 group-hover/run:opacity-100 group-has-[button:first-child:focus-visible]/run:visible group-has-[button:first-child:focus-visible]/run:opacity-100"
        >
          <span className="sr-only">카드 {run.cards.length}장: {summaryWords(summary)}.</span>
          <ul className="grid gap-0.5" aria-hidden="true">
            {run.cards.slice(0, TIP_CARDS).map((c) => (
              <li key={c.id} className="flex min-w-0 items-center gap-1.5 text-3xs">
                <AgentStateDot state={c.state} />
                <span className="truncate">{c.label}</span>
                <span className="ml-auto shrink-0 pl-1 text-muted-foreground">
                  {PROVIDER_SHORT[c.provider]} {STATE_WORD[c.state]}
                </span>
              </li>
            ))}
          </ul>
          {run.cards.length > TIP_CARDS && <p className="mt-0.5 text-3xs text-muted-foreground" aria-hidden="true">외 {run.cards.length - TIP_CARDS}장 — 실행을 누르면 오른쪽 패널에 모두 보입니다</p>}
        </div>
      )}
    </div>
  );
}

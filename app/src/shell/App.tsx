import { MessageSquarePlus, PanelRightOpen, SquareArrowOutUpRight, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";

import { isSessionRun, nextUp, type NextItem } from "../../../src/core/attention.ts";
import { listedRuns, ownerOf, sameFolder } from "../../../src/core/projects.ts";
import {
  changeProjects,
  LEAD_WORD,
  projectsFrom,
  projectsOf,
  useLeads,
  useProjectGroups,
  useProjectList,
  useProjectSource,
  type Project,
} from "../data/projects.ts";
import { lastProject, markOnboardingDone, onboardingDone, rememberProject, settingsNow } from "../data/settings.ts";
import { useLead } from "../data/useLead.ts";
import { useRunSource } from "../data/useLiveRuns.ts";
import { readParams, writeParam, type PanelView, type ScreenState } from "../data/useRun.ts";
import { Composer } from "../features/chat/Composer.tsx";
import { LeadGate } from "../features/chat/LeadGate.tsx";
import { LeadThread } from "../features/chat/LeadThread.tsx";
import { Thread } from "../features/chat/Thread.tsx";
import { WhoIsWorking } from "../features/office/WhoIsWorking.tsx";
import { ImportCard, useImportOffer } from "../features/projects/ImportCard.tsx";
import { NewRequest } from "../features/projects/NewRequest.tsx";
import { registerFolder } from "../features/projects/ProjectForms.tsx";
import { AccountMenu, SETTINGS_BUTTON_ID } from "../features/settings/AccountMenu.tsx";
import { Onboarding } from "../features/settings/Onboarding.tsx";
import type { SettingsSection } from "../features/settings/sections/index.ts";
import { Settings } from "../features/settings/Settings.tsx";
import { AllNextUp } from "../features/work/AllNextUp.tsx";
import { UpdatePrompt } from "../features/updates/UpdatePrompt.tsx";
import { actionCount, useHandled } from "../features/work/NextUp.tsx";
import { useNow } from "../features/work/WorkTab.tsx";
import { ICON_BUTTON } from "../lib/format.ts";
import { backend } from "../platform/backend/index.ts";
import { demoBackend } from "../platform/backend/demo.ts";
import { useAnnouncer } from "./announcer.ts";
import { usePanelLayout, usePopoutChannel, type Selection } from "./panelLayout.ts";
import { SidePanel } from "./SidePanel.tsx";
import { Sidebar } from "./Sidebar.tsx";
import { StatusBar } from "./StatusBar.tsx";

const initial = readParams();
/** The machine side (agents, about): the recorded demo answers with fixed data. */
const api = backend(!window.__TAURI_INTERNALS__ && new URLSearchParams(window.location.search).get("source") === "demo");

type Screen = "app" | "onboarding" | "settings" | "nextup" | "new";

export function App() {
  return initial.popout ? <PopoutPanel /> : <MainWindow />;
}

/**
 * Selection shared by the main window and the pop-out, mirrored into the URL.
 * `select` merges a change into the newest selection (even one made earlier in the
 * same event) and sends the merged whole to the other window; `receive` applies
 * what the other window sent without echoing it back.
 */
function useSelection(send: (selection: Selection) => void) {
  // Without a project in the link, the main window opens the one looked at last (Settings → 일반).
  const [selection, setSelection] = useState<Selection>(() => ({
    project: initial.project ?? (!initial.popout && settingsNow().start === "last" ? lastProject() : undefined),
    run: initial.run,
    view: initial.view,
    card: initial.card,
  }));
  const latest = useRef(selection);
  const apply = (next: Partial<Selection>) => {
    const merged = { ...latest.current, ...next };
    latest.current = merged;
    setSelection(merged);
    return merged;
  };
  useEffect(() => {
    writeParam("project", selection.project);
    writeParam("run", selection.run);
    writeParam("view", selection.view);
    writeParam("card", selection.card);
  }, [selection]);
  return {
    selection,
    select: (next: Partial<Selection>) => send(apply(next)),
    receive: (next: Selection) => void apply(next),
  };
}

type Run = ReturnType<typeof useRunSource>["runs"][number];

/**
 * What the selection points at: a listed project (the chosen one in any spelling, the
 * one holding the chosen run, else the newest), and the run shown in its lead chat
 * (the chosen one, else the project's newest).
 */
function resolve(runs: Run[], projects: Project[], selection: Selection) {
  const chosen = runs.find((r) => r.view.runId === selection.run);
  const wanted = selection.project ?? chosen?.view.project;
  const project = (wanted ? projects.find((p) => sameFolder(p.path, wanted)) ?? ownerOf(projects, wanted) : undefined) ?? projects[0];
  const current = chosen && (!project || ownerOf([project], chosen.view.project)) ? chosen : runs.find((r) => r.view.runId === project?.runs[0]?.runId);
  return { project, current };
}

/** The listed projects with their runs; every folder in history while the list cannot be read. */
function useProjects(runs: Run[], threads: { project: string; threadId: string }[], extra: (string | undefined)[]) {
  const { list, error } = useProjectList();
  const projects = useMemo(() => {
    const views = runs.map((r) => r.view);
    if (list) return projectsFrom(list, views, threads);
    if (!error) return [];
    const all = projectsOf(views);
    for (const path of extra) {
      if (path && !all.some((p) => sameFolder(p.path, path))) all.push({ path, name: path.split(/[\\/]/).filter(Boolean).pop() ?? path, kind: "history", runs: [] });
    }
    return all;
  }, [runs, threads, list, error, extra]);
  return { projects, list, error };
}

/** Orca's window: run sidebar, then the lead chat and a chosen view side by side, status bar below. */
function MainWindow() {
  const [chosenState, setState] = useState<ScreenState>(initial.state);
  const data = useRunSource(initial.run, initial.start);
  const { runs, replayingId, restart } = data;
  const listState = useProjectList();
  // The project list comes from this machine, or from the demo's own list when the screen shows the demo.
  useProjectSource(data.pending ? undefined : data.live ? api : demoBackend);
  // Live mode: loading until the first answer (runs and the project list), an error only when nothing could ever be read.
  const state: ScreenState =
    chosenState !== "full" ? chosenState : data.pending || listState.pending ? "loading" : data.live?.failed && runs.length === 0 ? "error" : "full";
  const layout = usePanelLayout((s) => receive(s));
  const { selection, select, receive } = useSelection(layout.broadcast);
  const askRef = useRef<HTMLTextAreaElement>(null);
  const groups = useProjectGroups();

  // The recorded demo keeps its own transcripts; everywhere else the lead chat is real (ADR-0008).
  const live = !data.pending && Boolean(data.live);
  const [threads, setThreads] = useState<{ project: string; threadId: string }[]>([]);
  const extra = useMemo(() => (live ? [...threads.map((t) => t.project), selection.project] : []), [live, threads, selection.project]);
  const { projects, list, error: listError } = useProjects(runs, threads, extra);
  const resolved = useMemo(() => resolve(runs, projects, selection), [runs, projects, selection]);
  const { project } = resolved;
  const lead = useLead(api, project?.path, live);
  const leadKey = JSON.stringify(lead.projects);
  useEffect(() => setThreads(JSON.parse(leadKey) as { project: string; threadId: string }[]), [leadKey]);
  const leads = useLeads(projects);
  const current = state === "full" ? resolved.current : undefined;
  const gateState = project ? leads.stateOf(project.path) : "ready";
  // A real lead has no resting gate: it answers when written to (the demo keeps the gate).
  const leadState = live ? (lead.busy || gateState === "working" ? "working" : "ready") : gateState;
  const announced = useAnnouncer(data.runs, current?.view);
  useEffect(() => rememberProject(project?.path), [project?.path]);
  const [headerError, setHeaderError] = useState<string>();
  useEffect(() => setHeaderError(undefined), [project?.path]);

  // First launch (or the "에이전트 연결 화면" start screen) shows agent onboarding instead of the chat.
  const [screen, setScreen] = useState<Screen>(() => (!onboardingDone() || settingsNow().start === "onboarding" ? "onboarding" : "app"));
  const [section, setSection] = useState<SettingsSection>("general");
  const settingsFrom = useRef<Screen>("app");
  const openSettings = (target: SettingsSection = "general") => {
    if (screen !== "settings") settingsFrom.current = screen;
    setSection(target);
    setScreen("settings");
  };
  const closeSettings = () => {
    setScreen(settingsFrom.current);
    // Back on the app, focus returns to the footer button that opened Settings.
    if (settingsFrom.current === "app") requestAnimationFrame(() => document.getElementById(SETTINGS_BUTTON_ID)?.focus());
  };
  // Esc leaves Settings, unless a menu or field inside it takes the key first (U10).
  useEffect(() => {
    if (screen !== "settings") return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || e.defaultPrevented) return;
      const target = e.target as HTMLElement | null;
      // An open menu on screen closes first (the app underneath stays mounted but hidden).
      const menuOpen = [...document.querySelectorAll<HTMLElement>("[aria-expanded=true]")].some((el) => el.offsetParent !== null);
      if (target?.closest("input, textarea, select, [contenteditable=true]") || menuOpen) return;
      closeSettings();
    };
    document.addEventListener("keydown", onKey);
    return () => document.removeEventListener("keydown", onKey);
  });
  const finishOnboarding = () => {
    markOnboardingDone();
    setScreen("app");
    compose();
  };
  const source = data.pending ? "pending" : data.live ? "live" : "demo";

  const projectOf = (run: string) => runs.find((r) => r.view.runId === run)?.view.project;
  const openCard = (run: string, card: string) => {
    const path = projectOf(run);
    select({ project: path, run, card, view: "work" });
    if (path) leads.wake(path);
    if (!layout.popped) layout.setOpen(true);
  };
  const wake = (path: string) => {
    if (!project || !sameFolder(path, project.path)) select({ project: path, run: undefined, card: undefined });
    leads.wake(path);
  };
  const compose = () => requestAnimationFrame(() => askRef.current?.focus());
  const openProject = (path: string) => {
    setScreen("app");
    select({ project: path, run: undefined, card: undefined });
    compose();
  };
  const retry = () => {
    setState("full");
    writeParam("state", undefined);
  };
  const showPanel = layout.open && !layout.popped;

  // Only listed projects show, and the counts follow them; history elsewhere is kept, unshown (ADR-0009).
  const now = useNow(data.clock);
  const { handled } = useHandled();
  const runViews = useMemo(() => runs.map((r) => r.view), [runs]);
  const views = useMemo(() => (state !== "full" ? [] : list ? listedRuns(list.projects, runViews) : runViews), [runViews, state, list]);
  // The badge counts what waits on a person: not a note to read, nothing from a Claude Code session (B1, U3).
  const nextCount = useMemo(() => {
    const { todo, stopped } = nextUp(
      views.filter((v) => !isSessionRun(v)),
      now,
      handled,
    );
    return actionCount([...todo, ...stopped]);
  }, [views, now, handled]);
  const openItem = (item: NextItem) => {
    setScreen("app");
    const card = item.cardId ?? runs.find((r) => r.view.runId === item.runId)?.view.cards[0]?.id;
    if (card) openCard(item.runId, card);
    else select({ project: projectOf(item.runId), run: item.runId, card: undefined });
  };

  // The day's cap and the running cap count O's cards only, as the policy does (B1).
  const oRuns = useMemo(() => (state === "full" ? runViews.filter((v) => !isSessionRun(v)) : []), [runViews, state]);
  const threadFolders = useMemo(() => threads.map((t) => t.project), [threads]);
  const offer = useImportOffer(list, runViews, threadFolders, live && state === "full");

  const unregister = async (path: string) => {
    try {
      await changeProjects((backendApi) => backendApi.projectsRemove(path));
    } catch (e) {
      setHeaderError(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <div className="grid h-full grid-cols-[16rem_minmax(0,1fr)] grid-rows-[minmax(0,1fr)_auto] max-md:grid-cols-[minmax(0,1fr)]">
      <a
        href="#"
        onClick={(e) => {
          e.preventDefault();
          setScreen("app");
          compose();
        }}
        className="sr-only focus:not-sr-only focus:fixed focus:top-2 focus:left-2 focus:z-50 focus:rounded-md focus:bg-popover focus:px-3 focus:py-2 focus:text-xs focus:text-popover-foreground focus:shadow-lg focus-ring"
      >
        대화 입력으로 건너뛰기
      </a>
      <UpdatePrompt api={api} />
      {/* Settings takes the sidebar's and main area's place; both stay mounted underneath to keep their state. */}
      {screen === "settings" && (
        <Settings api={api} section={section} source={source} onSection={setSection} onBack={closeSettings} onOnboarding={() => setScreen("onboarding")} />
      )}
      <div className="contents max-md:hidden" hidden={screen === "settings"}>
        <Sidebar
          state={state}
          projects={projects}
          groups={groups}
          // A real lead is ready whenever written to; it works while its runs go or while it answers.
          leadState={live ? (path) => (leads.stateOf(path) === "working" || (lead.busy && path === project?.path) ? "working" : "ready") : leads.stateOf}
          project={project?.path}
          run={current?.view.runId}
          card={selection.card}
          now={now}
          listError={listError}
          root={list?.root}
          // Picking something in the sidebar leaves onboarding for the chat (onboarding stays unfinished).
          onProject={(path) => {
            setScreen("app");
            select({ project: path, run: undefined, card: undefined });
          }}
          onWake={(path) => {
            setScreen("app");
            wake(path);
          }}
          onRun={(path, run) => {
            setScreen("app");
            select({ project: path, run, card: undefined });
            leads.wake(path);
          }}
          onCard={(run, card) => {
            setScreen("app");
            openCard(run, card);
          }}
          onNewRequest={() => setScreen("new")}
          onListed={openProject}
          onUnregister={(path) => void unregister(path)}
          nextCount={nextCount}
          nextOpen={screen === "nextup"}
          newOpen={screen === "new"}
          onNextUp={() => setScreen("nextup")}
          footer={
            <AccountMenu
              onSettings={() => openSettings()}
              onAbout={() => openSettings("about")}
              onAgents={() => setScreen("onboarding")}
              onRefreshUsage={data.live ? data.live.refreshUsage : undefined}
            />
          }
        />
      </div>
      {screen === "onboarding" && <Onboarding api={api} onDone={finishOnboarding} onSettings={() => openSettings("agents")} />}
      {screen === "nextup" && (
        <AllNextUp
          runs={views}
          now={now}
          loading={state === "loading"}
          onOpen={openItem}
          onProject={(path) => {
            setScreen("app");
            select({ project: path, run: undefined, card: undefined });
          }}
        />
      )}
      {screen === "new" && <NewRequest projects={projects} current={project?.path} onPick={openProject} />}
      <main ref={layout.container} hidden={screen !== "app"} className="flex min-h-0 min-w-0 max-lg:flex-col">
        <section className="flex min-h-0 min-w-0 flex-1 flex-col" aria-label="총괄과의 대화">
          <header className="flex h-9 shrink-0 items-center gap-2 border-b bg-tab-bar px-4 text-xs">
            <span className="min-w-0 truncate font-medium text-tab-active-foreground" title={project?.path}>
              {project ? `${project.name} 총괄` : "총괄"}
            </span>
            {project && <span className="shrink-0 rounded-sm bg-muted px-1 text-3xs whitespace-nowrap text-muted-foreground">{lead.busy ? "답하는 중" : LEAD_WORD[leadState]}</span>}
            {/* The demo's chat is one run's transcript; a real lead chat spans runs, so no run title rides next to it (U12). */}
            {!live && current && (leadState === "working" || leadState === "ready") && <span className="min-w-0 truncate text-muted-foreground">{current.view.title}</span>}
            <span className="ml-auto flex shrink-0 items-center gap-1">
              {project && <WhoIsWorking runs={project.runs} onOpen={openCard} />}
              {live && project && lead.items.length > 0 && (
                <button
                  type="button"
                  onClick={() => {
                    setHeaderError(undefined);
                    lead.startNew().then(compose, (e: unknown) => setHeaderError(`새 대화를 시작하지 못했습니다: ${e instanceof Error ? e.message : String(e)}`));
                  }}
                  disabled={lead.busy}
                  title="이 프로젝트의 총괄과 새 대화를 시작합니다 (지난 대화는 남습니다)"
                  className="flex h-6 items-center gap-1 rounded-md border px-2 whitespace-nowrap hover:bg-accent focus-ring disabled:cursor-not-allowed disabled:opacity-50"
                >
                  <MessageSquarePlus className="size-3.5 shrink-0" aria-hidden="true" />새 대화
                </button>
              )}
              {layout.popped && (
                <button type="button" onClick={layout.dock} title="별도 창에서 보는 중, 다시 붙이기" className="flex h-6 items-center gap-1 rounded-md border px-2 whitespace-nowrap hover:bg-accent focus-ring">
                  <SquareArrowOutUpRight className="size-3.5 shrink-0" aria-hidden="true" />
                  다시 붙이기
                </button>
              )}
              {!layout.popped && !layout.open && (
                <button type="button" onClick={() => layout.setOpen(true)} aria-label="작업 보기 패널 열기" title="작업 보기 패널 열기" className={ICON_BUTTON}>
                  <PanelRightOpen className="size-3.5" aria-hidden="true" />
                </button>
              )}
            </span>
          </header>
          {headerError && (
            <p role="alert" className="flex shrink-0 items-start gap-2 border-b bg-destructive/10 px-4 py-1.5 text-xs text-destructive">
              <span className="min-w-0 flex-1">{headerError}</span>
              <button type="button" onClick={() => setHeaderError(undefined)} aria-label="알림 닫기" className="shrink-0 rounded focus-ring">
                <X className="size-3.5" aria-hidden="true" />
              </button>
            </p>
          )}
          {list && offer.candidates.length > 0 && (
            <ImportCard
              root={list.root}
              candidates={offer.candidates}
              onDismiss={offer.dismiss}
              onDone={(first) => {
                offer.dismiss();
                if (first) openProject(first);
              }}
            />
          )}
          {live && state !== "error" ? (
            <div key={project?.path ?? "none"} className="bloom flex min-h-0 flex-1 flex-col">
              <LeadThread
                project={project?.path}
                lead={lead}
                runOf={(runId) => runs.find((r) => r.view.runId === runId)?.view}
                onOpenCard={openCard}
                onCompose={compose}
                onStartProject={async (path) => {
                  // A folder started here goes on the list too, or it would vanish from the sidebar (ADR-0009).
                  const listed = await registerFolder(path).catch(() => path);
                  await api.leadNew(listed);
                  openProject(listed);
                }}
              />
            </div>
          ) : state === "full" && project && (leadState === "resting" || leadState === "loading") ? (
            <LeadGate project={project} loading={leadState === "loading"} onWake={() => leads.wake(project.path)} />
          ) : (
            <div key={project?.path} className="bloom flex min-h-0 flex-1 flex-col">
              <Thread
                state={state}
                messages={current?.messages ?? []}
                run={current?.view}
                onOpenCard={(id) => current && openCard(current.view.runId, id)}
                onRetry={retry}
                onCompose={compose}
              />
            </div>
          )}
          <Composer inputRef={askRef} api={api} project={project?.path} lead={lead} onAgents={() => setScreen("onboarding")} />
        </section>
        {showPanel && (
          <>
            <div
              role="separator"
              aria-orientation="vertical"
              aria-label="패널 너비 조절 (화살표 키)"
              aria-valuenow={Math.round(layout.width)}
              tabIndex={0}
              onPointerDown={layout.startResize}
              onKeyDown={layout.resizeByKey}
              // Focused, the 4px rule shows as a full-height ring with a grip in the middle (P5).
              className="relative w-1 shrink-0 cursor-col-resize border-l bg-transparent hover:bg-ring/30 focus-visible:z-10 focus-visible:bg-ring focus-visible:outline-2 focus-visible:outline-offset-1 focus-visible:outline-ring max-lg:hidden after:absolute after:top-1/2 after:left-1/2 after:hidden after:h-8 after:w-1.5 after:-translate-x-1/2 after:-translate-y-1/2 after:rounded-full after:bg-foreground/70 focus-visible:after:block"
            />
            {/* The stored width can outgrow a window made narrower since; the chat keeps 360px (MIN_CHAT). */}
            <div className="min-h-0 shrink-0 max-lg:h-[50vh] max-lg:w-full! max-lg:border-t lg:max-w-[calc(100%-360px)]" style={{ width: layout.width }}>
              <SidePanel
                run={current?.view}
                projectRuns={project?.runs ?? []}
                usageRuns={runs.map((r) => r.view)}
                usageWindows={data.usageWindows}
                now={data.clock}
                leadResting={leadState === "resting" || leadState === "loading"}
                view={selection.view}
                card={selection.card}
                onView={(view: PanelView) => select({ view })}
                onCard={(target) => (target ? openCard(target.run, target.card) : select({ card: undefined }))}
                onClose={() => layout.setOpen(false)}
                onPopout={() => layout.popout({ ...selection, project: project?.path, run: current?.view.runId }, source === "pending" ? undefined : source)}
              />
            </div>
          </>
        )}
      </main>
      <StatusBar state={state} runs={oRuns} live={data.live} replaying={Boolean(replayingId)} onReplay={() => current && restart(current.view.runId)} />
      <p className="sr-only" aria-live="polite">
        {announced}
      </p>
    </div>
  );
}

/** The right panel alone in its own window, following and steering the main window's selection. */
function PopoutPanel() {
  const data = useRunSource(initial.run, 1);
  const { runs, usageWindows, clock } = data;
  useProjectSource(data.pending ? undefined : data.live ? api : demoBackend);
  const send = usePopoutChannel((s) => receive(s));
  const { selection, select, receive } = useSelection(send);
  const { projects } = useProjects(runs, [], []);
  const { project, current } = resolve(runs, projects, selection);
  // The main window's run until this window has read it: never quietly another run (B15).
  const waiting = Boolean(selection.run) && !runs.some((r) => r.view.runId === selection.run);
  useEffect(() => {
    document.title = waiting ? "불러오는 중 · project_O" : `${current?.view.title ?? "작업"} 보기 · project_O`;
  }, [current, waiting]);
  if (waiting) {
    return (
      <div className="grid h-full place-items-center p-6 text-center text-xs text-muted-foreground" role="status" aria-busy={data.pending}>
        {data.pending ? "본창에서 고른 실행을 불러오는 중…" : "본창에서 고른 실행을 이 창에서 찾지 못했습니다. 본창에서 다시 골라 주세요."}
      </div>
    );
  }
  return (
    <div className="h-full">
      <SidePanel
        run={current?.view}
        projectRuns={project?.runs ?? []}
        usageRuns={runs.map((r) => r.view)}
        usageWindows={usageWindows}
        now={clock}
        view={selection.view}
        card={selection.card}
        onView={(view) => select({ view })}
        onCard={(target) =>
          select(target ? { project: runs.find((r) => r.view.runId === target.run)?.view.project, run: target.run, card: target.card, view: "work" } : { card: undefined })
        }
      />
    </div>
  );
}

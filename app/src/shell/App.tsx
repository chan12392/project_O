import { MessageSquarePlus, PanelRightOpen, SquareArrowOutUpRight } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";

import { nextUp, type NextItem } from "../../../src/core/attention.ts";
import { LEAD_WORD, projectsOf, useLeads, useProjectGroups, type Project } from "../data/projects.ts";
import { lastProject, markOnboardingDone, onboardingDone, rememberProject, settingsNow } from "../data/settings.ts";
import { sameFolder, useLead } from "../data/useLead.ts";
import { useRunSource } from "../data/useLiveRuns.ts";
import { readParams, writeParam, type PanelView, type ScreenState } from "../data/useRun.ts";
import { Composer } from "../features/chat/Composer.tsx";
import { LeadGate } from "../features/chat/LeadGate.tsx";
import { LeadThread } from "../features/chat/LeadThread.tsx";
import { Thread } from "../features/chat/Thread.tsx";
import { WhoIsWorking } from "../features/office/WhoIsWorking.tsx";
import { AccountMenu, SETTINGS_BUTTON_ID } from "../features/settings/AccountMenu.tsx";
import { Onboarding } from "../features/settings/Onboarding.tsx";
import type { SettingsSection } from "../features/settings/sections/index.ts";
import { Settings } from "../features/settings/Settings.tsx";
import { AllNextUp } from "../features/work/AllNextUp.tsx";
import { UpdatePrompt } from "../features/updates/UpdatePrompt.tsx";
import { useHandled } from "../features/work/NextUp.tsx";
import { useNow } from "../features/work/WorkTab.tsx";
import { ICON_BUTTON } from "../lib/format.ts";
import { backend } from "../platform/backend/index.ts";
import { useAnnouncer } from "./announcer.ts";
import { usePanelLayout, usePopoutChannel, type Selection } from "./panelLayout.ts";
import { SidePanel } from "./SidePanel.tsx";
import { Sidebar } from "./Sidebar.tsx";
import { StatusBar } from "./StatusBar.tsx";

const initial = readParams();
/** The machine side (agents, about): the recorded demo answers with fixed data. */
const api = backend(!window.__TAURI_INTERNALS__ && new URLSearchParams(window.location.search).get("source") === "demo");

type Screen = "app" | "onboarding" | "settings" | "nextup";

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
 * What the selection points at: a project (the run's, else the newest), and the
 * run shown in its lead chat (the chosen one, else the project's newest).
 */
function resolve(runs: Run[], selection: Selection, extra: (string | undefined)[] = []) {
  const projects = projectsOf(runs.map((r) => r.view));
  // Live: folders with a lead thread but no run yet, and a folder just chosen, are projects too.
  for (const path of extra) {
    if (path && !projects.some((p) => sameFolder(p.path, path))) projects.push({ path, name: path.split(/[\\/]/).filter(Boolean).pop() ?? path, runs: [] });
  }
  const chosen = runs.find((r) => r.view.runId === selection.run);
  const project: Project | undefined = projects.find((p) => p.path === (selection.project ?? chosen?.view.project)) ?? projects[0];
  const current = chosen && chosen.view.project === project?.path ? chosen : runs.find((r) => r.view.runId === project?.runs[0]?.runId);
  return { projects, project, current };
}

/** Orca's window: run sidebar, then the lead chat and a chosen view side by side, status bar below. */
function MainWindow() {
  const [chosenState, setState] = useState<ScreenState>(initial.state);
  const data = useRunSource(initial.run, initial.start);
  const { runs, replayingId, restart } = data;
  // Live mode: loading until the first answer, an error only when nothing could ever be read.
  const state: ScreenState =
    chosenState !== "full" ? chosenState : data.pending ? "loading" : data.live?.failed && runs.length === 0 ? "error" : "full";
  const layout = usePanelLayout((s) => receive(s));
  const { selection, select, receive } = useSelection(layout.broadcast);
  const askRef = useRef<HTMLTextAreaElement>(null);
  const groups = useProjectGroups();

  // The recorded demo keeps its own transcripts; everywhere else the lead chat is real (ADR-0008).
  const live = !data.pending && Boolean(data.live);
  const [leadProjects, setLeadProjects] = useState<string[]>([]);
  const resolved = useMemo(
    () => resolve(runs, selection, live ? [...leadProjects, selection.project] : []),
    [runs, selection, live, leadProjects],
  );
  const { projects, project } = resolved;
  const lead = useLead(api, project?.path, live);
  const leadKey = lead.projects.map((p) => p.project).join("\n");
  useEffect(() => setLeadProjects(leadKey ? leadKey.split("\n") : []), [leadKey]);
  const leads = useLeads(projects);
  const current = state === "full" ? resolved.current : undefined;
  const gateState = project ? leads.stateOf(project.path) : "ready";
  // A real lead has no resting gate: it answers when written to (the demo keeps the gate).
  const leadState = live ? (lead.busy || gateState === "working" ? "working" : "ready") : gateState;
  const announced = useAnnouncer(data.runs, current?.view);
  useEffect(() => rememberProject(project?.path), [project?.path]);

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
    if (path !== project?.path) select({ project: path, run: undefined, card: undefined });
    leads.wake(path);
  };
  const compose = () => requestAnimationFrame(() => askRef.current?.focus());
  const retry = () => {
    setState("full");
    writeParam("state", undefined);
  };
  const showPanel = layout.open && !layout.popped;

  // Next up across every project: the sidebar's count and the view it opens.
  const now = useNow(data.clock);
  const { handled } = useHandled();
  const views = useMemo(() => (state === "full" ? runs.map((r) => r.view) : []), [runs, state]);
  const nextCount = useMemo(() => {
    const { todo, stopped } = nextUp(views, now, handled);
    return todo.length + stopped.length;
  }, [views, now, handled]);
  const openItem = (item: NextItem) => {
    setScreen("app");
    const card = item.cardId ?? runs.find((r) => r.view.runId === item.runId)?.view.cards[0]?.id;
    if (card) openCard(item.runId, card);
    else select({ project: projectOf(item.runId), run: item.runId, card: undefined });
  };

  return (
    <div className="grid h-full grid-cols-[16rem_minmax(0,1fr)] grid-rows-[minmax(0,1fr)_auto] max-md:grid-cols-[minmax(0,1fr)]">
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
          onCompose={() => {
            setScreen("app");
            compose();
          }}
          nextCount={nextCount}
          nextOpen={screen === "nextup"}
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
      <main ref={layout.container} hidden={screen !== "app"} className="flex min-h-0 min-w-0 max-lg:flex-col">
        <section className="flex min-h-0 min-w-0 flex-1 flex-col" aria-label="총괄과의 대화">
          <header className="flex h-9 shrink-0 items-center gap-2 border-b bg-tab-bar px-4 text-xs">
            <span className="shrink-0 font-medium text-tab-active-foreground">{project ? `${project.name} 총괄` : "총괄"}</span>
            {project && <span className="shrink-0 rounded-sm bg-muted px-1 text-3xs text-muted-foreground">{lead.busy ? "답하는 중" : LEAD_WORD[leadState]}</span>}
            {current && (leadState === "working" || leadState === "ready") && <span className="truncate text-muted-foreground">{current.view.title}</span>}
            <span className="ml-auto flex items-center gap-1">
              {project && <WhoIsWorking runs={project.runs} onOpen={openCard} />}
              {live && project && lead.items.length > 0 && (
                <button
                  type="button"
                  onClick={() => void lead.startNew().then(compose)}
                  disabled={lead.busy}
                  title="이 프로젝트의 총괄과 새 대화를 시작합니다 (지난 대화는 남습니다)"
                  className="flex h-6 items-center gap-1 rounded-md border px-2 hover:bg-accent focus-ring disabled:cursor-not-allowed disabled:opacity-50"
                >
                  <MessageSquarePlus className="size-3.5" aria-hidden="true" />새 대화
                </button>
              )}
              {layout.popped && (
                <button type="button" onClick={layout.dock} className="flex h-6 items-center gap-1 rounded-md border px-2 hover:bg-accent focus-ring">
                  <SquareArrowOutUpRight className="size-3.5" aria-hidden="true" />
                  별도 창에서 보는 중, 다시 붙이기
                </button>
              )}
              {!layout.popped && !layout.open && (
                <button type="button" onClick={() => layout.setOpen(true)} aria-label="작업 보기 패널 열기" title="작업 보기 패널 열기" className={ICON_BUTTON}>
                  <PanelRightOpen className="size-3.5" aria-hidden="true" />
                </button>
              )}
            </span>
          </header>
          {live && state !== "error" ? (
            <div key={project?.path ?? "none"} className="bloom flex min-h-0 flex-1 flex-col">
              <LeadThread
                project={project?.path}
                lead={lead}
                runOf={(runId) => runs.find((r) => r.view.runId === runId)?.view}
                onOpenCard={openCard}
                onCompose={compose}
                onStartProject={async (path) => {
                  await api.leadNew(path);
                  select({ project: path, run: undefined, card: undefined });
                  compose();
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
              className="w-1 shrink-0 cursor-col-resize border-l bg-transparent hover:bg-ring/30 focus-visible:bg-ring/50 focus-visible:outline-none max-lg:hidden"
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
      <StatusBar
        state={state}
        runs={state === "full" ? runs.map((r) => r.view) : []}
        live={data.live}
        replaying={Boolean(replayingId)}
        onReplay={() => current && restart(current.view.runId)}
      />
      <p className="sr-only" aria-live="polite">
        {announced}
      </p>
    </div>
  );
}

/** The right panel alone in its own window, following and steering the main window's selection. */
function PopoutPanel() {
  const { runs, usageWindows, clock } = useRunSource(initial.run, 1);
  const send = usePopoutChannel((s) => receive(s));
  const { selection, select, receive } = useSelection(send);
  const { project, current } = resolve(runs, selection);
  useEffect(() => {
    document.title = `${current?.view.title ?? "작업"} 보기 · project_O`;
  }, [current]);
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

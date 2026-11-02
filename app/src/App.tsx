import { PanelRightOpen, SquareArrowOutUpRight } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";

import type { RunView } from "../../src/core/snapshot.ts";
import { backend } from "./backend.ts";
import { AccountMenu, SETTINGS_BUTTON_ID } from "./components/AccountMenu.tsx";
import { Composer } from "./components/Composer.tsx";
import { LeadGate } from "./components/LeadGate.tsx";
import { Onboarding } from "./components/Onboarding.tsx";
import { Settings, type SettingsSection } from "./components/Settings.tsx";
import { SidePanel } from "./components/SidePanel.tsx";
import { Sidebar } from "./components/Sidebar.tsx";
import { StatusBar } from "./components/StatusBar.tsx";
import { Thread } from "./components/Thread.tsx";
import { WhoIsWorking } from "./components/WhoIsWorking.tsx";
import { ICON_BUTTON, STATE_WORD } from "./format.ts";
import { usePanelLayout, usePopoutChannel, type Selection } from "./panelLayout.ts";
import { LEAD_WORD, projectsOf, useLeads, useProjectGroups, type Project } from "./projects.ts";
import { useRunSource } from "./useLiveRuns.ts";
import { readParams, writeParam, type PanelView, type ScreenState } from "./useRun.ts";
import { lastProject, markOnboardingDone, onboardingDone, rememberProject, settingsNow } from "./settings.ts";

const initial = readParams();
/** The machine side (agents, about): the recorded demo answers with fixed data. */
const api = backend(new URLSearchParams(window.location.search).get("source") === "demo");

type Screen = "app" | "onboarding" | "settings";

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
function resolve(runs: Run[], selection: Selection) {
  const projects = projectsOf(runs.map((r) => r.view));
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

  const resolved = useMemo(() => resolve(runs, selection), [runs, selection]);
  const { projects, project } = resolved;
  const leads = useLeads(projects);
  const current = state === "full" ? resolved.current : undefined;
  const lead = project ? leads.stateOf(project.path) : "ready";
  const announced = useAnnouncer(current?.view);
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

  return (
    <div className="grid h-full grid-cols-[16rem_minmax(0,1fr)] grid-rows-[minmax(0,1fr)_auto] max-md:grid-cols-[minmax(0,1fr)]">
      {/* Settings takes the sidebar's and main area's place; both stay mounted underneath to keep their state. */}
      {screen === "settings" && (
        <Settings api={api} section={section} source={source} onSection={setSection} onBack={closeSettings} onOnboarding={() => setScreen("onboarding")} />
      )}
      <div className="contents max-md:hidden" hidden={screen === "settings"}>
        <Sidebar
          state={state}
          projects={projects}
          groups={groups}
          leadState={leads.stateOf}
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
      <main ref={layout.container} hidden={screen !== "app"} className="flex min-h-0 min-w-0 max-lg:flex-col">
        <section className="flex min-h-0 min-w-0 flex-1 flex-col" aria-label="총괄과의 대화">
          <header className="flex h-9 shrink-0 items-center gap-2 border-b bg-tab-bar px-4 text-xs">
            <span className="shrink-0 font-medium text-tab-active-foreground">{project ? `${project.name} 총괄` : "총괄"}</span>
            {project && <span className="shrink-0 rounded-sm bg-muted px-1 text-3xs text-muted-foreground">{LEAD_WORD[lead]}</span>}
            {current && (lead === "working" || lead === "ready") && <span className="truncate text-muted-foreground">{current.view.title}</span>}
            <span className="ml-auto flex items-center gap-1">
              {project && <WhoIsWorking runs={project.runs} onOpen={openCard} />}
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
          {state === "full" && project && (lead === "resting" || lead === "loading") ? (
            <LeadGate project={project} loading={lead === "loading"} onWake={() => leads.wake(project.path)} />
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
          <Composer inputRef={askRef} />
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
                leadResting={lead === "resting" || lead === "loading"}
                view={selection.view}
                card={selection.card}
                onView={(view: PanelView) => select({ view })}
                onCard={(target) => (target ? openCard(target.run, target.card) : select({ card: undefined }))}
                onClose={() => layout.setOpen(false)}
                onPopout={() => layout.popout({ ...selection, project: project?.path, run: current?.view.runId })}
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

/** Settings → 알림: a desktop notification for a finished card while the window is in the background. */
function notify(title: string, body: string) {
  if (!settingsNow().notify || typeof Notification === "undefined" || Notification.permission !== "granted" || document.hasFocus()) return;
  try {
    new Notification(title, { body });
  } catch {
    // Some webviews only allow notifications from a service worker; the screen reader line still speaks.
  }
}

/** Says out loud (and, if enabled, notifies) when a card finishes while its run is watched; cards that had already finished stay quiet. */
function useAnnouncer(run: RunView | undefined) {
  const [text, setText] = useState("");
  const seen = useRef(new Set<string>());
  const runs = useRef(new Set<string>());
  useEffect(() => {
    if (!run) return;
    const first = !runs.current.has(run.runId);
    runs.current.add(run.runId);
    for (const card of run.cards) {
      const key = `${run.runId}/${card.id}`;
      if (card.endedAt && !seen.current.has(key)) {
        seen.current.add(key);
        if (first) continue;
        const said = `${card.label} 카드 ${STATE_WORD[card.state]}`;
        setText(said);
        notify(run.title, said);
      }
    }
  }, [run]);
  return text;
}

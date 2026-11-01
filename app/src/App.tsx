import { PanelRightOpen, SquareArrowOutUpRight } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import type { RunView } from "../../src/core/snapshot.ts";
import { Composer } from "./components/Composer.tsx";
import { SidePanel } from "./components/SidePanel.tsx";
import { Sidebar } from "./components/Sidebar.tsx";
import { StatusBar } from "./components/StatusBar.tsx";
import { Thread } from "./components/Thread.tsx";
import { STATE_WORD } from "./format.ts";
import { usePanelLayout, usePopoutChannel, type Selection } from "./panelLayout.ts";
import { readParams, useDemoRuns, writeParam, type PanelView, type ScreenState } from "./useRun.ts";

const initial = readParams();

export function App() {
  return initial.popout ? <PopoutPanel /> : <MainWindow />;
}

/** Selection shared by the main window and the pop-out, mirrored into the URL. */
function useSelection(defaultRun: string | undefined) {
  const [selection, setSelection] = useState<Selection>({ run: initial.run ?? defaultRun, view: initial.view, card: initial.card });
  /** Applies a change from this window or the other one, and mirrors it into this window's URL. */
  const update = (next: Partial<Selection>) =>
    setSelection((s) => {
      const merged = { ...s, ...next };
      writeParam("run", merged.run);
      writeParam("view", merged.view);
      writeParam("card", merged.card);
      return merged;
    });
  return { selection, update };
}

/** Orca's window: run sidebar, then the lead chat and a chosen view side by side, status bar below. */
function MainWindow() {
  const [state, setState] = useState<ScreenState>(initial.state);
  const { runs, replayingId, restart } = useDemoRuns(initial.run, initial.start);
  const { selection, update } = useSelection(runs[0]?.view.runId);
  const layout = usePanelLayout(update);
  const askRef = useRef<HTMLTextAreaElement>(null);

  const current = state === "full" ? runs.find((r) => r.view.runId === selection.run) ?? runs[0] : undefined;
  const announced = useAnnouncer(current?.view);

  const select = (next: Partial<Selection>) => {
    update(next);
    layout.broadcast({ ...selection, ...next });
  };
  const openCard = (run: string, card: string) => {
    select({ run, card, view: "work" });
    if (!layout.popped) layout.setOpen(true);
  };
  const compose = () => requestAnimationFrame(() => askRef.current?.focus());
  const retry = () => {
    setState("full");
    writeParam("state", undefined);
  };
  const showPanel = layout.open && !layout.popped;

  return (
    <div className="grid h-full grid-cols-[16rem_minmax(0,1fr)] grid-rows-[minmax(0,1fr)_auto] max-md:grid-cols-[minmax(0,1fr)]">
      <div className="contents max-md:hidden">
        <Sidebar
          state={state}
          runs={runs.map((r) => r.view)}
          selected={current?.view.runId}
          card={selection.card}
          onRun={(run) => select({ run, card: undefined })}
          onCard={openCard}
          onCompose={compose}
        />
      </div>
      <main ref={layout.container} className="flex min-h-0 min-w-0 max-lg:flex-col">
        <section className="flex min-h-0 min-w-0 flex-1 flex-col" aria-label="총괄과의 대화">
          <header className="flex h-9 shrink-0 items-center gap-2 border-b bg-tab-bar px-4 text-xs">
            <span className="font-medium text-tab-active-foreground">총괄</span>
            {current && <span className="truncate text-muted-foreground">{current.view.title}</span>}
            <span className="ml-auto flex items-center gap-1">
              {layout.popped && (
                <button type="button" onClick={layout.dock} className="flex h-6 items-center gap-1 rounded-md border px-2 hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none">
                  <SquareArrowOutUpRight className="size-3.5" aria-hidden="true" />
                  별도 창에서 보는 중, 다시 붙이기
                </button>
              )}
              {!layout.popped && !layout.open && (
                <button
                  type="button"
                  onClick={() => layout.setOpen(true)}
                  aria-label="작업 보기 패널 열기"
                  title="작업 보기 패널 열기"
                  className="flex size-7 items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
                >
                  <PanelRightOpen className="size-3.5" aria-hidden="true" />
                </button>
              )}
            </span>
          </header>
          <Thread
            state={state}
            messages={current?.messages ?? []}
            run={current?.view}
            onOpenCard={(id) => current && openCard(current.view.runId, id)}
            onRetry={retry}
            onCompose={compose}
          />
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
            <div className="min-h-0 shrink-0 max-lg:h-[50vh] max-lg:w-full! max-lg:border-t" style={{ width: layout.width }}>
              <SidePanel
                run={current?.view}
                usageRuns={runs.map((r) => r.view)}
                view={selection.view}
                card={selection.card}
                onView={(view: PanelView) => select({ view })}
                onCard={(card) => select({ card })}
                onClose={() => layout.setOpen(false)}
                onPopout={() => layout.popout(selection)}
              />
            </div>
          </>
        )}
      </main>
      <StatusBar state={state} runs={state === "full" ? runs.map((r) => r.view) : []} replaying={Boolean(replayingId)} onReplay={() => current && restart(current.view.runId)} />
      <p className="sr-only" aria-live="polite">
        {announced}
      </p>
    </div>
  );
}

/** The right panel alone in its own window, following and steering the main window's selection. */
function PopoutPanel() {
  const { runs } = useDemoRuns(initial.run, 1);
  const { selection, update } = useSelection(runs[0]?.view.runId);
  const send = usePopoutChannel(update);
  const current = runs.find((r) => r.view.runId === selection.run) ?? runs[0];
  const select = (next: Partial<Selection>) => {
    update(next);
    send({ ...selection, ...next });
  };
  useEffect(() => {
    document.title = `${current?.view.title ?? "작업"} 보기 · project_O`;
  }, [current]);
  return (
    <div className="h-full">
      <SidePanel run={current?.view} usageRuns={runs.map((r) => r.view)} view={selection.view} card={selection.card} onView={(view) => select({ view })} onCard={(card) => select({ card })} />
    </div>
  );
}

/** Says out loud when a card finishes, for anyone looking elsewhere. */
function useAnnouncer(run: RunView | undefined) {
  const [text, setText] = useState("");
  const seen = useRef(new Set<string>());
  useEffect(() => {
    for (const card of run?.cards ?? []) {
      const key = `${run?.runId}/${card.id}`;
      if (card.endedAt && !seen.current.has(key)) {
        seen.current.add(key);
        setText(`${card.label} 카드 ${STATE_WORD[card.state]}`);
      }
    }
  }, [run]);
  return text;
}

import { useEffect, useRef, useState } from "react";

import type { RunView } from "../../src/core/snapshot.ts";
import { Composer } from "./components/Composer.tsx";
import { SidePanel } from "./components/SidePanel.tsx";
import { Sidebar } from "./components/Sidebar.tsx";
import { StatusBar } from "./components/StatusBar.tsx";
import { Thread } from "./components/Thread.tsx";
import { STATE_WORD } from "./format.ts";
import { readParams, useDemoRuns, writeParam, type PanelView, type ScreenState } from "./useRun.ts";

const initial = readParams();

/** Orca's window: run sidebar, then the lead chat and a chosen view side by side, status bar below. */
export function App() {
  const [state, setState] = useState<ScreenState>(initial.state);
  const { runs, replayingId, restart } = useDemoRuns(initial.run, initial.start);
  const [runId, setRunId] = useState(initial.run ?? runs[0]?.view.runId);
  const [view, setView] = useState<PanelView>(initial.view);
  const [card, setCard] = useState(initial.card);
  const askRef = useRef<HTMLTextAreaElement>(null);

  const current = state === "full" ? runs.find((r) => r.view.runId === runId) ?? runs[0] : undefined;
  const announced = useAnnouncer(current?.view);

  const chooseRun = (id: string) => {
    setRunId(id);
    setCard(undefined);
    writeParam("run", id);
    writeParam("card", undefined);
  };
  const chooseView = (next: PanelView) => {
    setView(next);
    writeParam("view", next);
  };
  const chooseCard = (id: string) => {
    setCard(id);
    writeParam("card", id);
  };
  const openCard = (run: string, id: string) => {
    chooseRun(run);
    chooseCard(id);
    chooseView("work");
  };
  const compose = () => requestAnimationFrame(() => askRef.current?.focus());
  const retry = () => {
    setState("full");
    writeParam("state", undefined);
  };

  return (
    <div className="grid h-full grid-cols-[16rem_minmax(0,1fr)] grid-rows-[minmax(0,1fr)_auto] max-md:grid-cols-[minmax(0,1fr)]">
      <div className="contents max-md:hidden">
        <Sidebar state={state} runs={runs.map((r) => r.view)} selected={current?.view.runId} card={card} onRun={chooseRun} onCard={openCard} onCompose={compose} />
      </div>
      <main className="grid min-h-0 min-w-0 grid-cols-[minmax(22rem,1fr)_minmax(22rem,1fr)] max-lg:grid-cols-[minmax(0,1fr)] max-lg:grid-rows-[minmax(0,1fr)_minmax(0,1fr)]">
        <section className="flex min-h-0 min-w-0 flex-col" aria-label="총괄과의 대화">
          <header className="flex h-9 shrink-0 items-center gap-2 border-b bg-tab-bar px-4 text-xs">
            <span className="font-medium text-tab-active-foreground">총괄</span>
            {current && <span className="truncate text-muted-foreground">{current.view.title}</span>}
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
        <SidePanel run={current?.view} view={view} card={card} onView={chooseView} onCard={chooseCard} />
      </main>
      <StatusBar state={state} runs={state === "full" ? runs.map((r) => r.view) : []} replaying={Boolean(replayingId)} onReplay={() => current && restart(current.view.runId)} />
      <p className="sr-only" aria-live="polite">
        {announced}
      </p>
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

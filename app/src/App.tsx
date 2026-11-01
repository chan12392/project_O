import { useEffect, useRef, useState } from "react";

import { CardTranscript } from "./components/CardTranscript.tsx";
import { Composer } from "./components/Composer.tsx";
import { Sidebar } from "./components/Sidebar.tsx";
import { StatusBar } from "./components/StatusBar.tsx";
import { TabBar } from "./components/TabBar.tsx";
import { Thread } from "./components/Thread.tsx";
import { STATE_WORD } from "./format.ts";
import { readParams, useDemoRun, writeParam, type ScreenState } from "./useRun.ts";

const initial = readParams();

/** Orca's window layout: worktree sidebar, tab strip with panes, status bar. */
export function App() {
  const [state, setState] = useState<ScreenState>(initial.state);
  const [tab, setTab] = useState(initial.tab);
  const { run, messages, replay } = useDemoRun(initial.start);
  const askRef = useRef<HTMLTextAreaElement>(null);
  const announced = useAnnouncer(run);

  const card = run?.cards.find((c) => c.id === tab);
  const openTab = (id: string) => {
    setTab(id);
    writeParam("tab", id === "lead" ? undefined : id);
  };
  const compose = () => {
    openTab("lead");
    requestAnimationFrame(() => askRef.current?.focus());
  };
  const retry = () => {
    setState("full");
    writeParam("state", undefined);
  };

  return (
    <div className="grid h-full grid-cols-[17rem_minmax(0,1fr)] grid-rows-[minmax(0,1fr)_auto] max-md:grid-cols-[minmax(0,1fr)]">
      <div className="contents max-md:hidden">
        <Sidebar state={state} run={run} tab={tab} onTab={openTab} onCompose={compose} />
      </div>
      <main className="flex min-h-0 min-w-0 flex-col">
        <TabBar run={state === "full" ? run : undefined} tab={card ? tab : "lead"} onTab={openTab} />
        {state === "full" && card ? (
          <CardTranscript card={card} />
        ) : (
          <>
            <Thread state={state} messages={messages} run={run} onOpenCard={openTab} onRetry={retry} onCompose={compose} />
            <Composer inputRef={askRef} />
          </>
        )}
      </main>
      <StatusBar state={state} run={state === "full" ? run : undefined} onReplay={replay} />
      <p className="sr-only" aria-live="polite">
        {announced}
      </p>
    </div>
  );
}

/** Says out loud when a card finishes, for anyone on another tab. */
function useAnnouncer(run: ReturnType<typeof useDemoRun>["run"]) {
  const [text, setText] = useState("");
  const seen = useRef(new Set<string>());
  useEffect(() => {
    for (const card of run?.cards ?? []) {
      if (card.endedAt && !seen.current.has(card.id)) {
        seen.current.add(card.id);
        setText(`${card.role} 카드 ${STATE_WORD[card.state]}`);
      }
    }
    if (!run) seen.current.clear();
  }, [run]);
  return text;
}

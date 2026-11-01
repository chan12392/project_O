import { useEffect, useRef, useState } from "react";

import { STATE_WORD } from "./format.ts";
import { Composer } from "./components/Composer.tsx";
import { Office } from "./components/Office.tsx";
import { Thread } from "./components/Thread.tsx";
import { readParams, useDemoRun, writeParam, type ScreenState } from "./useRun.ts";

const initial = readParams();

export function App() {
  const [state, setState] = useState<ScreenState>(initial.state);
  const [selected, setSelected] = useState<string | undefined>(initial.card);
  const { run, messages, playing, replay } = useDemoRun(initial.start);
  const askRef = useRef<HTMLTextAreaElement>(null);
  const announced = useAnnouncer(run);

  const select = (id: string) => {
    setSelected(id);
    writeParam("card", id);
  };
  const retry = () => {
    setState("full");
    writeParam("state", undefined);
  };

  return (
    <div className="app">
      <a className="skip" href="#ask">
        요청 입력으로 건너뛰기
      </a>
      <header className="bar">
        <h1 className="bar__mark">Project O</h1>
        <p className="bar__project">
          <span className="bar__label">프로젝트</span> {run?.project.split("/").pop() ?? "todo-app"}
        </p>
        <div className="bar__end">
          <p className="bar__preview">미리보기: 오늘 시험 실행 기록을 다시 보여 줍니다</p>
          {state === "full" && (
            <button type="button" className="btn btn--secondary btn--small" onClick={replay} disabled={playing && !run}>
              처음부터 다시 보기
            </button>
          )}
        </div>
      </header>
      <main className="panes" id="main">
        <section className="chat" aria-label="총괄과의 대화">
          <Thread
            state={state}
            messages={messages}
            run={run}
            selected={selected}
            onSelect={select}
            onRetry={retry}
            onCompose={() => askRef.current?.focus()}
          />
          <Composer inputRef={askRef} />
        </section>
        <Office state={state} run={run} selected={selected ?? run?.cards[0]?.id} onSelect={select} />
      </main>
      <p className="visually-hidden" aria-live="polite">
        {announced}
      </p>
    </div>
  );
}

/** Says out loud when a card finishes, for anyone not watching the office. */
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

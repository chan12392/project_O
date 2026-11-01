import { useEffect, useLayoutEffect, useRef } from "react";

import type { CardView, RunView } from "../../../src/core/snapshot.ts";
import { PROVIDER_NAME, STATE_WORD, clip, modelLine, timeOf } from "../format.ts";
import type { Message, ScreenState } from "../useRun.ts";
import { Markdown } from "./Markdown.tsx";

interface Props {
  state: ScreenState;
  messages: Message[];
  run?: RunView;
  selected?: string;
  onSelect: (id: string) => void;
  onRetry: () => void;
  onCompose: () => void;
}

export function Thread({ state, messages, run, selected, onSelect, onRetry, onCompose }: Props) {
  useFollowBottom([messages, run]);
  if (state === "loading") {
    return (
      <div className="thread" aria-busy="true" aria-label="대화 불러오는 중">
        <div className="msg msg--me msg--skeleton" />
        <div className="msg msg--lead msg--skeleton" />
        <div className="runsheet runsheet--skeleton" />
      </div>
    );
  }
  if (state === "error") {
    return (
      <div className="thread">
        <section className="notice" aria-labelledby="error-title">
          <h2 id="error-title">기록을 읽지 못했습니다</h2>
          <p>실행 기록 파일(events.jsonl)을 열 수 없습니다. 파일이 없거나 아직 쓰는 중일 수 있습니다.</p>
          <p>터미널에서 o status로 실행이 남아 있는지 확인한 뒤 다시 불러오세요.</p>
          <button type="button" className="btn btn--primary" onClick={onRetry}>
            다시 불러오기
          </button>
        </section>
      </div>
    );
  }
  if (state === "empty" || messages.length === 0) {
    return (
      <div className="thread">
        <section className="notice" aria-labelledby="empty-title">
          <h2 id="empty-title">아직 맡긴 일이 없습니다</h2>
          <p>총괄에게 할 일을 적어 보내면 작업카드로 나뉘어 각 에이전트에게 전달됩니다.</p>
          <button type="button" className="btn btn--primary" onClick={onCompose}>
            첫 요청 쓰기
          </button>
        </section>
      </div>
    );
  }
  return (
    <div className="thread">
      {messages.map((m) => (
        <article key={m.at} className={`msg msg--${m.from}`} aria-label={m.from === "me" ? "내 요청" : "총괄의 답"}>
          <header className="msg__head">
            <span className="msg__who">{m.from === "me" ? "나" : "총괄 Claude"}</span>
            <time dateTime={m.at}>{timeOf(m.at)}</time>
          </header>
          <div className="msg__body">
            <Markdown text={m.text} />
          </div>
        </article>
      ))}
      {run && <RunSheet run={run} selected={selected} onSelect={onSelect} />}
    </div>
  );
}

/** Keeps the newest message in view, unless the reader has scrolled up to read. */
function useFollowBottom(deps: unknown[]) {
  const follow = useRef(true);
  useEffect(() => {
    const onScroll = () => {
      const root = document.documentElement;
      follow.current = root.scrollHeight - window.scrollY - window.innerHeight < 80;
    };
    window.addEventListener("scroll", onScroll, { passive: true });
    return () => window.removeEventListener("scroll", onScroll);
  }, []);
  useLayoutEffect(() => {
    if (follow.current) window.scrollTo({ top: document.documentElement.scrollHeight });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);
}

const RUN_WORD: Record<RunView["status"], string> = { running: "진행 중", done: "모두 완료", failed: "실패한 카드 있음", stopped: "중지됨" };

function RunSheet({ run, selected, onSelect }: { run: RunView; selected?: string; onSelect: (id: string) => void }) {
  const current = run.cards.find((c) => c.id === selected) ?? run.cards[0];
  return (
    <section className="runsheet" aria-labelledby="run-title">
      <header className="runsheet__head">
        <h2 id="run-title">{run.title}</h2>
        <p className="runsheet__status" role="status">
          {RUN_WORD[run.status]}
        </p>
      </header>
      <table className="cards">
        <caption className="visually-hidden">작업카드 {run.cards.length}장</caption>
        <colgroup>
          <col className="cards__col-card" />
          <col className="cards__col-state" />
          <col />
        </colgroup>
        <thead>
          <tr>
            <th scope="col">카드</th>
            <th scope="col">상태</th>
            <th scope="col">최근 활동</th>
          </tr>
        </thead>
        <tbody>
          {run.cards.map((card) => (
            <CardRow key={card.id} card={card} selected={card.id === current?.id} onSelect={onSelect} />
          ))}
        </tbody>
      </table>
      {current && <CardLog card={current} />}
    </section>
  );
}

function CardRow({ card, selected, onSelect }: { card: CardView; selected: boolean; onSelect: (id: string) => void }) {
  return (
    <tr className={`is-${card.state}${selected ? " is-selected" : ""}`}>
      <th scope="row">
        <button type="button" className="cards__pick" aria-pressed={selected} onClick={() => onSelect(card.id)}>
          {card.role}
        </button>
        <span className="cards__who">
          <span>{PROVIDER_NAME[card.provider]}</span> <span className="nowrap">{modelLine(card.model, card.effort)}</span>
        </span>
      </th>
      <td className="cards__state">{STATE_WORD[card.state]}</td>
      <td>
        <span className="cards__activity">{card.activity ? clip(card.activity, 44) : "아직 기록 없음"}</span>
      </td>
    </tr>
  );
}

function CardLog({ card }: { card: CardView }) {
  const entries = card.log.slice(-8).reverse();
  return (
    <div className="cardlog">
      <h3>
        {card.role} 카드 기록
      </h3>
      {entries.length === 0 ? (
        <p className="cardlog__empty">아직 남긴 기록이 없습니다.</p>
      ) : (
        <ol className="cardlog__list" reversed>
          {entries.map((entry, i) => (
            <li key={`${entry.at}-${i}`} className={`cardlog__entry is-${entry.kind}`}>
              <time dateTime={entry.at}>{timeOf(entry.at)}</time>
              <span>{entry.text.split("\n")[0]}</span>
            </li>
          ))}
        </ol>
      )}
    </div>
  );
}

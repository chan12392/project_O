import { useEffect, useState, type CSSProperties } from "react";

import { LIMITS } from "../../../src/core/policy.ts";
import type { RunView } from "../../../src/core/snapshot.ts";
import { PROVIDER_NAME } from "../format.ts";
import type { ScreenState } from "../useRun.ts";
import { Station } from "./Station.tsx";

interface Props {
  state: ScreenState;
  run?: RunView;
  selected?: string;
  onSelect: (id: string) => void;
}

/**
 * The office: one desk per work card. Typing loops run past five seconds, so
 * they pause on hover and focus, carry a visible pause control and stay still
 * under reduced motion.
 */
export function Office({ state, run, selected, onSelect }: Props) {
  const [held, setHeld] = useState(() => matchMedia("(prefers-reduced-motion: reduce)").matches);
  const [hover, setHover] = useState(false);
  const [focus, setFocus] = useState(false);
  const anyRunning = run?.cards.some((c) => c.state === "running") ?? false;

  useEffect(() => {
    const query = matchMedia("(prefers-reduced-motion: reduce)");
    const sync = () => setHeld((h) => h || query.matches);
    query.addEventListener("change", sync);
    return () => query.removeEventListener("change", sync);
  }, []);

  return (
    <section className="office" aria-labelledby="office-title">
      <header className="office__head">
        <h2 id="office-title">사무실</h2>
        {state === "full" && anyRunning && (
          <button type="button" className="btn btn--secondary btn--small" onClick={() => setHeld((h) => !h)}>
            {held ? "움직임 켜기" : "움직임 멈추기"}
          </button>
        )}
      </header>

      <div
        className={`room${!held && !hover && !focus ? " is-running" : ""}`}
        onPointerEnter={() => setHover(true)}
        onPointerLeave={() => setHover(false)}
        onFocus={() => setFocus(true)}
        onBlur={() => setFocus(false)}
      >
        <div className="room__wall">
          <p className="board" aria-label="오늘 맡긴 일">
            {BOARD[state](run)}
          </p>
        </div>
        <div
          className="room__floor"
          role="group"
          aria-label="작업 현황 다이어그램, 카드마다 책상 하나"
          aria-busy={state === "loading"}
          style={{ "--desks": Math.min(4, Math.max(1, state === "full" ? (run?.cards.length ?? 1) : 3)) } as CSSProperties}
        >
          {state === "loading" && [0, 1, 2].map((i) => <div key={i} className="station station--skeleton" aria-hidden="true" />)}
          {state === "full" && run?.cards.map((card) => <Station key={card.id} card={card} selected={card.id === selected} onSelect={onSelect} />)}
          {state === "empty" && <p className="room__note">책상이 비어 있습니다. 총괄에게 일을 맡기면 카드마다 책상이 생깁니다.</p>}
          {state === "error" && <p className="room__note">기록을 다시 불러오면 책상이 돌아옵니다.</p>}
        </div>
      </div>

      <Ledger run={state === "full" ? run : undefined} known={state === "full" || state === "empty"} />
    </section>
  );
}

const BOARD: Record<ScreenState, (run?: RunView) => string> = {
  full: (run) => run?.title ?? "오늘 맡긴 일 없음",
  empty: () => "오늘 맡긴 일 없음",
  loading: () => " ",
  error: () => "기록을 읽지 못함",
};

/**
 * Today's figures against the dispatch policy's ceilings (src/core/policy.ts).
 * While loading or after an error the figures are unknown, and say so.
 */
function Ledger({ run, known }: { run?: RunView; known: boolean }) {
  const count = (n: number) => (known ? String(n) : "?");
  const started = run?.cards.filter((c) => c.startedAt).length ?? 0;
  const active = run?.cards.filter((c) => c.state === "running").length ?? 0;
  const limited = [...new Set(run?.cards.flatMap((c) => c.log.filter((l) => l.kind === "limit").map(() => PROVIDER_NAME[c.provider])) ?? [])];
  return (
    <dl className="ledger">
      <div>
        <dt>오늘 시작한 카드</dt>
        <dd>
          <span className="figure">{count(started)}</span>
          <span className="ledger__of">하루 최대 {LIMITS.maxCardsPerDay}</span>
        </dd>
      </div>
      <div>
        <dt>지금 실행 중</dt>
        <dd>
          <span className="figure">{count(active)}</span>
          <span className="ledger__of">동시 최대 {LIMITS.maxActiveTotal}</span>
        </dd>
      </div>
      <div>
        <dt>한도에 걸린 서비스</dt>
        <dd>
          <span className="figure">{count(limited.length)}</span>
          <span className="ledger__of">{!known ? "확인 전" : limited.length > 0 ? limited.join(", ") : "걸린 곳 없음"}</span>
        </dd>
      </div>
    </dl>
  );
}

import { Pause, Play } from "lucide-react";
import { useState } from "react";

import type { CardView, RunView } from "../../../src/core/snapshot.ts";
import { PROVIDER_NAME, STATE_WORD, clip, modelLine } from "../format.ts";
import { HEIGHT, POSES, TYPING, WIDTH, type Rect } from "../sprite.ts";
import { AgentStateDot } from "./AgentStateDot.tsx";

const REDUCED = typeof matchMedia !== "undefined" && matchMedia("(prefers-reduced-motion: reduce)").matches;

/** The office: one desk per card, each agent drawn in its card's real state. */
export function Office({ run, selected, onSelect }: { run: RunView; selected?: string; onSelect: (id: string) => void }) {
  const [held, setHeld] = useState(REDUCED);
  const anyRunning = run.cards.some((c) => c.state === "running");
  return (
    <div className="scrollbar-sleek min-h-0 flex-1 overflow-y-auto bg-chat-canvas">
      <div className="flex items-center justify-between px-5 pt-4">
        <p className="text-xs text-muted-foreground">카드마다 책상 하나. 상태는 실행 기록에서 읽습니다.</p>
        {anyRunning && (
          <button
            type="button"
            onClick={() => setHeld((h) => !h)}
            className="flex h-7 items-center gap-1.5 rounded-md border px-2 text-xs hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
          >
            {held ? <Play className="size-3.5" aria-hidden="true" /> : <Pause className="size-3.5" aria-hidden="true" />}
            {held ? "움직임 켜기" : "움직임 멈추기"}
          </button>
        )}
      </div>
      <div
        role="group"
        aria-label="작업 현황 다이어그램, 카드마다 책상 하나"
        className={`grid grid-cols-[repeat(auto-fill,minmax(11rem,1fr))] gap-4 p-5 ${held ? "" : "office-running"}`}
      >
        {run.cards.map((card) => (
          <Desk key={card.id} card={card} selected={card.id === selected} onSelect={onSelect} />
        ))}
      </div>
    </div>
  );
}

function Pixels({ rects }: { rects: Rect[] }) {
  return (
    <>
      {rects.map((r, i) => (
        <rect key={i} className={`px-${r.key}`} x={r.x} y={r.y} width={r.w} height={1} />
      ))}
    </>
  );
}

function Desk({ card, selected, onSelect }: { card: CardView; selected: boolean; onSelect: (id: string) => void }) {
  return (
    <button
      type="button"
      aria-pressed={selected}
      onClick={() => onSelect(card.id)}
      className={`desk desk--${card.provider} is-${card.state} flex flex-col gap-3 rounded-xl border bg-card p-3 text-left text-card-foreground hover:bg-accent/30 focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none aria-pressed:border-ring`}
    >
      <span className="flex justify-center rounded-lg bg-muted/40 px-2 pt-3">
        <svg className="w-full max-w-40" viewBox={`0 0 ${WIDTH} ${HEIGHT}`} shapeRendering="crispEdges" aria-hidden="true">
          {card.state === "running" ? (
            <>
              <g className="frame-a">
                <Pixels rects={TYPING[0]} />
              </g>
              <g className="frame-b">
                <Pixels rects={TYPING[1]} />
              </g>
            </>
          ) : (
            <Pixels rects={card.state === "done" ? POSES.done : POSES.rest} />
          )}
        </svg>
      </span>
      <span className="flex min-w-0 flex-col gap-1">
        <span className="flex items-center gap-1.5 text-sm font-medium">
          <AgentStateDot state={card.state} />
          <span className="truncate">{card.label}</span>
          <span className="ml-auto shrink-0 text-xs text-muted-foreground">{STATE_WORD[card.state]}</span>
        </span>
        <span className="truncate font-mono text-xs text-muted-foreground">
          {PROVIDER_NAME[card.provider]} {modelLine(card.model, card.effort)}
        </span>
        {card.activity && <span className="line-clamp-2 text-xs text-muted-foreground">{clip(card.activity, 72)}</span>}
      </span>
    </button>
  );
}

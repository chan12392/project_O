import { Users } from "lucide-react";
import { useCallback, useRef, useState } from "react";

import type { CardView, RunView } from "../../../src/core/snapshot.ts";
import { useDismiss } from "../dismiss.ts";
import { PROVIDER_SHORT, STATE_WORD, clip, timeOf } from "../format.ts";
import { AgentStateDot } from "./AgentStateDot.tsx";
import { Avatar } from "./Avatar.tsx";

const RECENT = 6;

/**
 * Pattern from beakho-design lucian-074 (header dropdown with a count badge):
 * who in this project is working on what, at a glance from the chat header.
 */
export function WhoIsWorking({ runs, onOpen }: { runs: RunView[]; onOpen: (run: string, card: string) => void }) {
  const [open, setOpen] = useState(false);
  const panel = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  const close = useCallback(() => setOpen(false), []);
  useDismiss(panel, open, close, button);

  const cards = runs.flatMap((run) => run.cards.map((card) => ({ run, card })));
  const working = cards.filter(({ card }) => card.state === "running" || card.state === "queued");
  const recent = cards
    .filter(({ card }) => card.endedAt)
    .sort((a, b) => (b.card.endedAt ?? "").localeCompare(a.card.endedAt ?? ""))
    .slice(0, RECENT);

  return (
    <div className="relative">
      <button
        ref={button}
        type="button"
        onClick={() => setOpen((o) => !o)}
        aria-expanded={open}
        aria-controls="who-is-working"
        aria-label={`누가 무엇을 하는지 보기, 작업 중 ${working.length}명`}
        title="누가 무엇을 하는지"
        className="relative flex size-7 items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none aria-expanded:bg-accent aria-expanded:text-foreground"
      >
        <Users className="size-3.5" aria-hidden="true" />
        {working.length > 0 && (
          <span className="absolute -top-0.5 -right-0.5 grid h-3.5 min-w-3.5 place-items-center rounded-full bg-agent-working px-0.5 text-3xs leading-none font-semibold text-black" aria-hidden="true">
            {working.length}
          </span>
        )}
      </button>
      {open && (
        <div
          ref={panel}
          id="who-is-working"
          role="dialog"
          aria-label="누가 무엇을 하는지"
          className="bloom absolute top-full right-0 z-30 mt-1.5 w-80 max-w-[calc(100vw-2rem)] overflow-hidden rounded-xl border bg-popover text-popover-foreground shadow-xl"
        >
          <div className="flex items-center justify-between border-b px-3.5 py-2.5">
            <span className="text-xs font-semibold">지금 누가 무엇을</span>
            <span className="text-2xs text-muted-foreground">작업 중 {working.length} · 최근 끝남 {recent.length}</span>
          </div>
          <div className="scrollbar-sleek max-h-[60vh] overflow-y-auto">
            {working.length === 0 && recent.length === 0 && <p className="px-3.5 py-4 text-xs text-muted-foreground">이 프로젝트에 맡긴 카드가 없습니다.</p>}
            {[...working, ...recent].map(({ run, card }) => (
              <Row key={`${run.runId}/${card.id}`} run={run} card={card} onOpen={() => (onOpen(run.runId, card.id), close())} />
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

function Row({ run, card, onOpen }: { run: RunView; card: CardView; onOpen: () => void }) {
  const live = card.state === "running" || card.state === "queued";
  return (
    <button
      type="button"
      onClick={onOpen}
      className="flex w-full gap-2.5 border-b px-3.5 py-2.5 text-left last:border-b-0 hover:bg-accent/60 focus-visible:bg-accent focus-visible:outline-none data-live:bg-agent-working/5"
      data-live={live || undefined}
    >
      <Avatar speaker={card.provider} size="sm" />
      <span className="min-w-0 flex-1">
        <span className="flex items-center gap-1.5 text-xs">
          <span className="truncate font-medium">{card.label}</span>
          <span className="shrink-0 text-muted-foreground">{PROVIDER_SHORT[card.provider]}</span>
          <span className="ml-auto flex shrink-0 items-center gap-1 text-2xs text-muted-foreground">
            <AgentStateDot state={card.state} />
            {STATE_WORD[card.state]}
          </span>
        </span>
        <span className="mt-0.5 block truncate text-2xs text-muted-foreground">{clip(card.activity || run.title, 80)}</span>
        <span className="mt-0.5 block text-3xs text-muted-foreground tabular-nums">
          {run.title} · {timeOf(card.startedAt)}
          {card.endedAt && `–${timeOf(card.endedAt)}`}
        </span>
      </span>
    </button>
  );
}

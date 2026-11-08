import { Columns2, Grid2x2, Square } from "lucide-react";
import { useState } from "react";

import type { CardView, RunView } from "../../../../src/core/snapshot.ts";
import { EMPTY_COMPOSER, PANE_COUNTS, defaultPaneCount, fitPaneCount, fitPanes, showInPane, withComposer, type Composer, type PaneCount } from "../../../../src/core/panes.ts";
import { CardTranscript } from "../chat/CardTranscript.tsx";
import { lookOf } from "../office/characters.ts";
import { Avatar } from "../office/Avatar.tsx";
import { AgentStateDot } from "../../ui/AgentStateDot.tsx";
import { STATE_WORD, names } from "../../lib/format.ts";
import { readStored, writeStored } from "../../lib/storage.ts";
import type { Backend } from "../../platform/backend/index.ts";

const COUNT_KEY = "project-o:agent-panes";
const COUNT_ICON = { 1: Square, 2: Columns2, 4: Grid2x2 } as const;
const COUNT_LABEL = { 1: "한 화면", 2: "두 화면", 4: "네 화면" } as const;
/** Grid shape per pane count, by the width the view itself gets (the side panel takes its share):
 * side by side from 42rem; stacked below, where each pane keeps a readable height and the view scrolls. */
const STACKED = "grid-cols-1 auto-rows-[minmax(18rem,1fr)] overflow-y-auto @2xl:overflow-hidden";
const GRID = {
  1: "grid-cols-1 grid-rows-1",
  2: `${STACKED} @2xl:grid-cols-2 @2xl:grid-rows-1`,
  4: `${STACKED} @2xl:grid-cols-2 @2xl:grid-rows-2`,
} as const;

type Pane = { run: RunView; card: CardView };

/** A team's agents as tabs, with one, two or four of them open side by side. */
export function AgentPanes({ title, panes, api, onClose }: { title: string; panes: Pane[]; api?: Backend; onClose: () => void }) {
  const ids = panes.map((p) => p.card.id);
  const [chosen, setChosen] = useState<PaneCount | undefined>(() => readStored<PaneCount | undefined>(COUNT_KEY, undefined, (saved) => PANE_COUNTS.find((c) => c === saved)));
  // Two panes need two agents and four need three; a saved split larger than the team shrinks to fit.
  const count = fitPaneCount(chosen ?? defaultPaneCount(ids.length), ids.length);
  const [picked, setPicked] = useState<string[]>([]);
  const [focus, setFocus] = useState(0);
  // Unsent follow-ups by card, kept while a card's pane is hidden (this view is keyed by run).
  const [composers, setComposers] = useState<Record<string, Composer>>({});
  const open = fitPanes(picked, ids, count);
  const focused = Math.min(focus, Math.max(open.length - 1, 0));
  const byId = new Map(panes.map((p) => [p.card.id, p]));

  const choose = (next: PaneCount) => {
    setChosen(next);
    writeStored(COUNT_KEY, next);
  };
  const openTab = (id: string) => {
    const next = showInPane(open, focused, id);
    setPicked(next.open);
    setFocus(next.focus);
  };

  return (
    <section aria-label="에이전트 작업" className="@container flex min-h-0 min-w-0 flex-1 flex-col bg-chat-canvas">
      <header className="flex h-9 shrink-0 items-center gap-2 border-b bg-tab-bar px-4 text-xs font-medium">
        <span className="min-w-0 truncate">{title}</span>
        <span className="ml-auto flex shrink-0 items-center gap-1">
          {ids.length > 1 && (
            <span role="group" aria-label="화면 나누기" className="flex items-center rounded-md border p-0.5">
              {PANE_COUNTS.filter((c) => c < 4 || ids.length >= 3).map((c) => {
                const Icon = COUNT_ICON[c];
                return (
                  <button key={c} type="button" onClick={() => choose(c)} aria-pressed={count === c} title={COUNT_LABEL[c]} className={`flex size-6 items-center justify-center rounded focus-ring ${count === c ? "bg-accent text-foreground" : "text-muted-foreground hover:bg-accent/60"}`}>
                    <Icon className="size-3.5" aria-hidden="true" />
                    <span className="sr-only">{COUNT_LABEL[c]}</span>
                  </button>
                );
              })}
            </span>
          )}
          <button type="button" onClick={onClose} className="rounded px-2 py-1 text-muted-foreground hover:bg-accent focus-ring">총괄 대화</button>
        </span>
      </header>
      {panes.length === 0 ? (
        <p className="p-4 text-sm text-muted-foreground">아직 표시할 에이전트가 없습니다.</p>
      ) : (
        <>
          {panes.length > 1 && (
            <div role="tablist" aria-label="에이전트" className="scrollbar-sleek flex shrink-0 gap-1 overflow-x-auto border-b bg-tab-bar px-2 py-1">
              {panes.map(({ run, card }) => {
                const at = open.indexOf(card.id);
                const shown = at >= 0;
                return (
                  <button
                    key={card.id}
                    type="button"
                    role="tab"
                    aria-selected={shown}
                    onClick={() => openTab(card.id)}
                    title={`${card.label} · ${STATE_WORD[card.state]}`}
                    className={`flex h-7 max-w-56 shrink-0 items-center gap-1.5 rounded-md px-2 text-xs focus-ring ${shown ? (at === focused ? "bg-accent text-foreground" : "bg-accent/50 text-foreground") : "text-muted-foreground hover:bg-accent/40"}`}
                  >
                    <AgentStateDot state={card.state} />
                    <Avatar speaker={card.provider} look={lookOf(run, card.id)} size="xs" />
                    <span className="min-w-0 truncate">{card.label}</span>
                    {shown && count > 1 && <span className="shrink-0 rounded-sm bg-background/60 px-1 font-mono text-3xs tabular-nums text-muted-foreground">{at + 1}</span>}
                  </button>
                );
              })}
            </div>
          )}
          <div className={`grid min-h-0 flex-1 ${GRID[count]}`}>
            {open.map((id, i) => {
              const pane = byId.get(id);
              if (!pane) return null;
              return (
                <div
                  key={id}
                  role="tabpanel"
                  aria-label={pane.card.label}
                  onPointerDown={() => setFocus(i)}
                  onFocusCapture={() => setFocus(i)}
                  className={`relative flex min-h-0 min-w-0 flex-col overflow-hidden border-b @2xl:border-r ${open.length > 1 && i === focused ? "after:pointer-events-none after:absolute after:inset-0 after:ring-1 after:ring-inset after:ring-ring/60" : ""}`}
                >
                  <CardTranscript card={pane.card} run={pane.run} names={names(pane.run)} api={api} onFollowup={() => {}} composer={{ ...(Object.hasOwn(composers, id) ? composers[id] : EMPTY_COMPOSER), set: (change) => setComposers((all) => withComposer(all, id, change)) }} />
                </div>
              );
            })}
          </div>
        </>
      )}
    </section>
  );
}

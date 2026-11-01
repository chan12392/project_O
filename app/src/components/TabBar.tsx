import { MessageSquare } from "lucide-react";

import type { RunView } from "../../../src/core/snapshot.ts";
import { PROVIDER_SHORT } from "../format.ts";
import { AgentStateDot } from "./AgentStateDot.tsx";

/** Orca's tab strip: the lead chat first, then one tab per work card. */
export function TabBar({ run, tab, onTab }: { run?: RunView; tab: string; onTab: (tab: string) => void }) {
  return (
    <div role="tablist" aria-label="열린 탭" className="scrollbar-sleek flex h-9 shrink-0 items-stretch overflow-x-auto border-b bg-tab-bar">
      <Tab id="lead" current={tab} onTab={onTab}>
        <MessageSquare className="size-3.5 text-muted-foreground" aria-hidden="true" />
        총괄
      </Tab>
      {run?.cards.map((card) => (
        <Tab key={card.id} id={card.id} current={tab} onTab={onTab}>
          <AgentStateDot state={card.state} />
          {card.role}
          <span className="text-muted-foreground">{PROVIDER_SHORT[card.provider]}</span>
        </Tab>
      ))}
    </div>
  );
}

function Tab({ id, current, onTab, children }: { id: string; current: string; onTab: (tab: string) => void; children: React.ReactNode }) {
  const active = id === current;
  return (
    <button
      type="button"
      role="tab"
      aria-selected={active}
      onClick={() => onTab(id)}
      className="relative flex shrink-0 items-center gap-1.5 border-r px-3 text-xs whitespace-nowrap text-tab-inactive-foreground hover:bg-accent/40 focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none focus-visible:ring-inset aria-selected:bg-tab-active aria-selected:text-tab-active-foreground"
    >
      {children}
      {active && <span className="absolute inset-x-0 bottom-0 h-px bg-tab-active-indicator" aria-hidden="true" />}
    </button>
  );
}

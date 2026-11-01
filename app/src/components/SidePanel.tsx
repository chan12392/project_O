import { Building2, ExternalLink, KanbanSquare, PanelRightClose } from "lucide-react";

import type { RunView } from "../../../src/core/snapshot.ts";
import type { UsageWindow } from "../../../src/core/usage.ts";
import type { PanelView } from "../useRun.ts";
import { Office } from "./Office.tsx";
import { TeamChat } from "./TeamChat.tsx";
import { UsagePanel } from "./UsagePanel.tsx";
import { WorkTab } from "./WorkTab.tsx";

const VIEWS: { id: PanelView; label: string; icon: typeof Building2 }[] = [
  { id: "work", label: "에이전트 작업", icon: KanbanSquare },
  { id: "office", label: "사무실", icon: Building2 },
];

interface Props {
  run?: RunView;
  /** The selected project's runs, shown on the work board. */
  projectRuns: RunView[];
  /** Runs whose tokens the usage row adds up (every project's: the subscription is shared). */
  usageRuns: RunView[];
  usageWindows: UsageWindow[];
  leadResting?: boolean;
  /** The clock tickets age by; a replayed run passes its own. Defaults to the wall clock. */
  now?: Date;
  view: PanelView;
  card?: string;
  onView: (view: PanelView) => void;
  /** Opens a card's record, or goes back to the board with undefined. */
  onCard: (target?: { run: string; card: string }) => void;
  /** Absent in the pop-out window, where the window itself is closed instead. */
  onClose?: () => void;
  onPopout?: () => void;
}

const ICON_BUTTON =
  "flex size-7 items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none";

/** The right half of the split: pick what to watch beside the lead chat. */
export function SidePanel({ run, projectRuns, usageRuns, usageWindows, leadResting, now, view, card, onView, onCard, onClose, onPopout }: Props) {
  return (
    <section className="flex h-full min-h-0 min-w-0 flex-col" aria-label="작업 보기">
      <div className="flex h-9 shrink-0 items-stretch border-b bg-tab-bar">
        <div role="tablist" aria-label="보기 고르기" className="flex min-w-0 items-stretch overflow-x-auto">
          {VIEWS.map(({ id, label, icon: Icon }) => (
            <button
              key={id}
              type="button"
              role="tab"
              aria-selected={view === id}
              onClick={() => onView(id)}
              className="relative flex items-center gap-1.5 border-r px-3 text-xs text-tab-inactive-foreground hover:bg-accent/40 focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none focus-visible:ring-inset aria-selected:bg-tab-active aria-selected:text-tab-active-foreground"
            >
              <Icon className="size-3.5" aria-hidden="true" />
              {label}
              {view === id && <span className="absolute inset-x-0 bottom-0 h-px bg-tab-active-indicator" aria-hidden="true" />}
            </button>
          ))}
        </div>
        <div className="ml-auto flex shrink-0 items-center gap-0.5 px-1.5">
          {onPopout && (
            <button type="button" onClick={onPopout} className={ICON_BUTTON} aria-label="별도 창으로 띄우기" title="별도 창으로 띄우기">
              <ExternalLink className="size-3.5" aria-hidden="true" />
            </button>
          )}
          {onClose && (
            <button type="button" onClick={onClose} className={ICON_BUTTON} aria-label="패널 닫기" title="패널 닫기">
              <PanelRightClose className="size-3.5" aria-hidden="true" />
            </button>
          )}
        </div>
      </div>

      {view === "work" && <WorkTab runs={projectRuns} open={run && card ? { run: run.runId, card } : undefined} onCard={onCard} now={now} />}

      {view === "office" && !run && <p className="p-5 text-sm text-muted-foreground">이 프로젝트에는 아직 작업이 없습니다.</p>}
      {view === "office" && run && (
        <div className="scrollbar-sleek min-h-0 flex-1 overflow-y-auto bg-chat-canvas pb-6">
          <Office run={run} selected={card} leadResting={leadResting} onSelect={(id) => (onCard({ run: run.runId, card: id }), onView("work"))} />
          <UsagePanel runs={usageRuns} windows={usageWindows} />
          <section aria-labelledby="team-title" className="mt-5">
            <h3 id="team-title" className="px-5 text-2xs font-semibold tracking-wider text-muted-foreground uppercase">
              팀 대화 {run.messages.length > 0 && <span className="ml-1 normal-case">{run.messages.length}</span>}
            </h3>
            <TeamChat run={run} embedded />
          </section>
        </div>
      )}
    </section>
  );
}

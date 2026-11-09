import { ExternalLink, PanelRightClose, type LucideIcon } from "lucide-react";
import type { ReactNode } from "react";

import type { RunView } from "../../../src/core/snapshot.ts";
import type { UsageWindow } from "../../../src/core/usage.ts";
import { PANEL_VIEWS, type PanelView } from "../data/useRun.ts";
import type { LeadState } from "../data/useLead.ts";
import { OFFICE_VIEW } from "../features/office/OfficeView.tsx";
import { WORK_VIEW } from "../features/work/WorkTab.tsx";
import { ICON_BUTTON } from "../lib/format.ts";

/** What a side panel view is drawn from. */
export interface PanelViewProps {
  run?: RunView;
  /** The selected project's runs, shown on the work board. */
  projectRuns: RunView[];
  /** Runs whose tokens the usage row adds up (every project's: the subscription is shared). */
  usageRuns: RunView[];
  usageWindows: UsageWindow[];
  leadResting?: boolean;
  lead?: LeadState;
  /** The clock tickets age by; a replayed run passes its own. Defaults to the wall clock. */
  now?: Date;
  card?: string;
  /** Opens a card's record in the work view, or goes back to the board with undefined. */
  onCard: (target?: { run: string; card: string }) => void;
  /** "새 설계"/"+": opens a new lead tab for this project and selects it; rejects with the reason (ADR-0034). */
  onNewLead?: () => Promise<void>;
}

/** One view of the side panel: its tab and what it draws. */
export interface PanelViewDef {
  label: string;
  icon: LucideIcon;
  render: (props: PanelViewProps) => ReactNode;
}

/** Every view by id; tabs follow PANEL_VIEWS (data/useRun.ts), which also reads `?view=`. */
const VIEWS: Record<PanelView, PanelViewDef> = { work: WORK_VIEW, office: OFFICE_VIEW };

interface Props extends PanelViewProps {
  view: PanelView;
  onView: (view: PanelView) => void;
  /** Absent in the pop-out window, where the window itself is closed instead. */
  onClose?: () => void;
  onPopout?: () => void;
}

/** The right half of the split: pick what to watch beside the lead chat. */
export function SidePanel({ view, onView, onClose, onPopout, ...shown }: Props) {
  return (
    <section className="flex h-full min-h-0 min-w-0 flex-col" aria-label="작업 보기">
      <div className="flex h-9 shrink-0 items-stretch border-b bg-tab-bar">
        <div role="group" aria-label="보기 고르기" className="flex min-w-0 items-stretch overflow-x-auto">
          {PANEL_VIEWS.map((id) => {
            const { label, icon: Icon } = VIEWS[id];
            return (
              <button
                key={id}
                type="button"
                aria-pressed={view === id}
                onClick={() => onView(id)}
                className="relative flex items-center gap-1.5 border-r px-3 text-xs text-tab-inactive-foreground hover:bg-accent/40 focus-ring focus-visible:ring-inset aria-pressed:bg-tab-active aria-pressed:text-tab-active-foreground"
              >
                <Icon className="size-3.5" aria-hidden="true" />
                {label}
                {view === id && <span className="absolute inset-x-0 bottom-0 h-px bg-tab-active-indicator" aria-hidden="true" />}
              </button>
            );
          })}
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

      {VIEWS[view].render(shown)}
    </section>
  );
}

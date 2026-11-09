import { Building2 } from "lucide-react";

import type { PanelViewDef } from "../../shell/SidePanel.tsx";
import { TeamChat } from "../chat/TeamChat.tsx";
import { UsagePanel } from "../usage/UsagePanel.tsx";
import { Office } from "./Office.tsx";

/** The side panel's "사무실" view: the run's people, the usage row and the team chat. */
export const OFFICE_VIEW: PanelViewDef = {
  label: "사무실",
  icon: Building2,
  render: ({ run, usageRuns, usageWindows, leadResting, card, onCard }) =>
    !run ? (
      <p className="p-5 text-sm text-muted-foreground">이 프로젝트에는 아직 작업이 없습니다.</p>
    ) : (
      <div className="scrollbar-sleek min-h-0 flex-1 overflow-y-auto bg-chat-canvas pb-6">
        <Office run={run} selected={card} leadResting={leadResting} onSelect={(id) => onCard({ run: run.runId, card: id })} />
        <UsagePanel runs={usageRuns} windows={usageWindows} />
        <section aria-labelledby="team-title" className="mt-5">
          <h3 id="team-title" className="px-5 text-2xs font-semibold tracking-wider text-muted-foreground uppercase">
            팀 대화 {run.messages.length > 0 && <span className="ml-1 normal-case">{run.messages.length}</span>}
          </h3>
          <TeamChat run={run} />
        </section>
      </div>
    ),
};

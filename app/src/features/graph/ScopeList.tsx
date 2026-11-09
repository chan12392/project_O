import { Folder, Waypoints } from "lucide-react";
import type { ReactNode } from "react";

import type { Project } from "../../data/projects.ts";
import type { GraphScope } from "./useGraphView.ts";

/**
 * The left column: "전체" for the whole workspace, the listed projects indented
 * under it. `current` earns a "현재" badge and a project without an index gets
 * "인덱스 없음" (remembered from the last overall view).
 */
export function ScopeList({ projects, scope, current, indexed, onPick }: {
  projects: Project[];
  scope: GraphScope;
  current?: string;
  /** "인덱스 없음" badges from the last overall view, keyed by project path. */
  indexed: Record<string, boolean>;
  onPick: (scope: GraphScope) => void;
}) {
  return (
    <nav className="scrollbar-sleek w-48 shrink-0 overflow-y-auto border-r py-2 pl-2" aria-label="그래프 범위">
      <ScopeButton icon={<Waypoints className="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />} active={scope.scope === "overall"} onClick={() => onPick({ scope: "overall" })}>
        전체
      </ScopeButton>
      <div className="ml-3 border-l pl-1">
        {projects.map((p) => (
          <ScopeButton
            key={p.path}
            icon={<Folder className="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />}
            active={scope.scope === "project" && scope.project === p.path}
            missing={p.missing}
            title={p.path}
            badge={indexed[p.path] === false ? "인덱스 없음" : current === p.path ? "현재" : undefined}
            onClick={() => onPick({ scope: "project", project: p.path })}
          >
            {p.name}
          </ScopeButton>
        ))}
        {projects.length === 0 && <p className="px-2 py-1.5 text-2xs text-muted-foreground">목록에 프로젝트가 없습니다.</p>}
      </div>
    </nav>
  );
}

/** One row of the scope list: "전체" at top level, projects indented under it. */
function ScopeButton({ icon, active, missing, title, badge, onClick, children }: { icon: ReactNode; active: boolean; missing?: boolean; title?: string; badge?: string; onClick: () => void; children: string }) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-current={active ? "page" : undefined}
      title={title}
      className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-[13px] hover:bg-sidebar-accent focus-ring aria-[current=page]:bg-sidebar-accent aria-[current=page]:font-medium"
    >
      {icon}
      <span className={`min-w-0 flex-1 truncate ${missing ? "text-muted-foreground line-through" : ""}`}>{children}</span>
      {badge && <span className="shrink-0 text-3xs text-muted-foreground">{badge}</span>}
    </button>
  );
}

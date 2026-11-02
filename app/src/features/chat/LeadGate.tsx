import { Coffee } from "lucide-react";

import { timeOf } from "../../lib/format.ts";
import type { Project } from "../../data/projects.ts";
import { Sprite } from "../office/Sprite.tsx";

/** The project's lead while it rests, and while its recent session loads after being called. */
export function LeadGate({ project, loading, onWake }: { project: Project; loading: boolean; onWake: () => void }) {
  const last = project.runs[0];
  if (loading) {
    // Placeholders shaped like the chat while loading, then the real thing fades in.
    return (
      <div className="min-h-0 flex-1 overflow-hidden bg-chat-canvas" aria-busy="true">
        <div className="mx-auto flex max-w-3xl flex-col gap-4 px-6 py-6">
          <p role="status" className="text-xs text-muted-foreground">
            {project.name} 총괄의 최근 세션 불러오는 중…
          </p>
          <div className="shimmer ml-auto h-14 w-3/5 rounded-lg" aria-hidden="true" />
          <div className="shimmer h-20 w-4/5 rounded-lg" aria-hidden="true" />
          <div className="shimmer h-24 w-full rounded-xl" aria-hidden="true" />
        </div>
      </div>
    );
  }
  return (
    <div className="grid min-h-0 flex-1 place-items-center overflow-y-auto bg-chat-canvas p-6">
      <div className="bloom flex max-w-sm flex-col items-center text-center">
        <button
          type="button"
          onClick={onWake}
          className="w-28 rounded-xl focus-ring"
          aria-label={`${project.name} 총괄 부르기`}
        >
          <Sprite speaker="lead" action="coffee" />
        </button>
        <h2 className="mt-3 text-sm font-semibold">{project.name} 총괄이 쉬는 중입니다</h2>
        <p className="mt-1 text-xs text-muted-foreground">
          {last ? (
            <>
              마지막 작업: {last.title} · {timeOf(last.startedAt)}
            </>
          ) : (
            "아직 맡긴 작업이 없습니다."
          )}
        </p>
        <button
          type="button"
          onClick={onWake}
          className="mt-4 flex h-8 items-center gap-1.5 rounded-md border bg-background px-3 text-xs hover:bg-accent focus-ring"
        >
          <Coffee className="size-3.5" aria-hidden="true" />
          총괄 부르기 · 최근 세션 불러오기
        </button>
      </div>
    </div>
  );
}

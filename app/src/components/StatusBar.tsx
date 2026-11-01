import { RotateCcw } from "lucide-react";

import { LIMITS } from "../../../src/core/policy.ts";
import type { RunView } from "../../../src/core/snapshot.ts";
import { PROVIDER_NAME } from "../format.ts";
import type { ScreenState } from "../useRun.ts";

/** Orca's bottom status bar, carrying today's figures against the dispatch ceilings. */
export function StatusBar({ state, run, onReplay }: { state: ScreenState; run?: RunView; onReplay: () => void }) {
  const known = state === "full" || state === "empty";
  const started = run?.cards.filter((c) => c.startedAt).length ?? 0;
  const active = run?.cards.filter((c) => c.state === "running").length ?? 0;
  const limited = [...new Set(run?.cards.flatMap((c) => c.log.filter((l) => l.kind === "limit").map(() => PROVIDER_NAME[c.provider])) ?? [])];
  const n = (value: number) => (known ? value : "?");

  return (
    <footer className="col-span-2 flex h-7 shrink-0 items-center gap-4 border-t bg-sidebar px-3 text-xs text-muted-foreground">
      <span>
        오늘 카드 <span className="text-foreground tabular-nums">{n(started)}</span> / {LIMITS.maxCardsPerDay}
      </span>
      <span>
        실행 중 <span className="text-foreground tabular-nums">{n(active)}</span> / {LIMITS.maxActiveTotal}
      </span>
      <span className="max-sm:hidden">{!known ? "한도 확인 전" : limited.length > 0 ? `한도 걸림: ${limited.join(", ")}` : "한도 걸린 서비스 없음"}</span>
      <span className="ml-auto max-md:hidden">미리보기: 오늘 시험 실행 기록 재생</span>
      {state === "full" && (
        <button
          type="button"
          onClick={onReplay}
          className="ml-auto flex h-5 items-center gap-1 rounded px-1.5 whitespace-nowrap md:ml-0 hover:bg-accent hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
        >
          <RotateCcw className="size-3" aria-hidden="true" />
          처음부터
        </button>
      )}
    </footer>
  );
}

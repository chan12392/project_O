import { RotateCcw } from "lucide-react";

import { LIMITS } from "../../../src/core/policy.ts";
import type { RunView } from "../../../src/core/snapshot.ts";
import { PROVIDER_NAME } from "../format.ts";
import type { ScreenState } from "../useRun.ts";

/** Orca's bottom status bar, carrying the day's figures against the dispatch ceilings. */
export function StatusBar({ state, runs, replaying, onReplay }: { state: ScreenState; runs: RunView[]; replaying: boolean; onReplay: () => void }) {
  const known = state === "full" || state === "empty";
  const cards = runs.flatMap((r) => r.cards);
  const started = cards.filter((c) => c.startedAt).length;
  const active = cards.filter((c) => c.state === "running").length;
  const limited = [...new Set(cards.flatMap((c) => c.log.filter((l) => l.kind === "limit").map(() => PROVIDER_NAME[c.provider])))];
  const n = (value: number) => (known ? value : "?");

  return (
    <footer className="col-span-2 flex h-7 shrink-0 items-center gap-4 border-t bg-sidebar px-3 text-xs text-muted-foreground max-md:col-span-1">
      <span>
        카드 <span className="text-foreground tabular-nums">{n(started)}</span> / 하루 {LIMITS.maxCardsPerDay}
      </span>
      <span>
        실행 중 <span className="text-foreground tabular-nums">{n(active)}</span> / {LIMITS.maxActiveTotal}
      </span>
      <span className="max-sm:hidden">{!known ? "한도 확인 전" : limited.length > 0 ? `한도 걸림: ${limited.join(", ")}` : "한도 걸린 서비스 없음"}</span>
      <span className="ml-auto max-md:hidden">{replaying ? "미리보기: 기록을 재생하는 중" : "미리보기: 이 PC의 실제 실행 기록"}</span>
      {state === "full" && (
        <button
          type="button"
          onClick={onReplay}
          className="ml-auto flex h-5 items-center gap-1 rounded px-1.5 whitespace-nowrap hover:bg-accent hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none md:ml-0"
        >
          <RotateCcw className="size-3" aria-hidden="true" />이 작업 다시 재생
        </button>
      )}
    </footer>
  );
}

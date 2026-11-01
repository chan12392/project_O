import { RefreshCw, RotateCcw } from "lucide-react";

import { LIMITS } from "../../../src/core/policy.ts";
import type { RunView } from "../../../src/core/snapshot.ts";
import { PROVIDER_NAME, timeOf } from "../format.ts";
import type { LiveStatus } from "../useLiveRuns.ts";
import type { ScreenState } from "../useRun.ts";

const BUTTON = "flex h-5 items-center gap-1 rounded px-1.5 whitespace-nowrap hover:bg-accent hover:text-foreground focus-ring";

function sameDay(at: string, now: Date): boolean {
  const day = new Date(at);
  return day.getFullYear() === now.getFullYear() && day.getMonth() === now.getMonth() && day.getDate() === now.getDate();
}

/**
 * Orca's bottom status bar, carrying the day's figures against the dispatch ceilings
 * and which source the screen shows: this machine's O runs (`live`) or the recorded demo.
 */
export function StatusBar({ state, runs, live, replaying, onReplay }: { state: ScreenState; runs: RunView[]; live?: LiveStatus; replaying: boolean; onReplay: () => void }) {
  const known = state === "full" || state === "empty";
  const cards = runs.flatMap((r) => r.cards);
  // Live runs span many days; only today's starts count against the daily cap.
  const now = new Date();
  const started = cards.filter((c) => c.startedAt && (!live || sameDay(c.startedAt, now))).length;
  const active = cards.filter((c) => c.state === "running").length;
  const limited = [...new Set(cards.flatMap((c) => c.log.filter((l) => l.kind === "limit").map(() => PROVIDER_NAME[c.provider])))];
  const n = (value: number) => (known ? value : "?");
  const usageAt = live?.usage?.at;
  const unavailable = live?.usage?.unavailable.map((u) => `${u.provider}: ${u.reason}`).join("\n");

  return (
    <footer className="col-span-2 flex h-7 shrink-0 items-center gap-4 border-t bg-sidebar px-3 text-xs text-muted-foreground max-md:col-span-1">
      <span>
        카드 <span className="text-foreground tabular-nums">{n(started)}</span> / 하루 {LIMITS.maxCardsPerDay}
      </span>
      <span>
        실행 중 <span className="text-foreground tabular-nums">{n(active)}</span> / {LIMITS.maxActiveTotal}
      </span>
      <span className="max-sm:hidden">{!known ? "한도 확인 전" : limited.length > 0 ? `한도 걸림: ${limited.join(", ")}` : "한도 걸린 서비스 없음"}</span>
      {live ? (
        <>
          <span className="ml-auto max-md:hidden">{live.failed ? "이 PC의 O 실행 기록 · 연결 끊김, 다시 시도하는 중" : "이 PC의 O 실행 기록 · 실시간"}</span>
          <button
            type="button"
            onClick={live.refreshUsage}
            disabled={live.refreshing}
            aria-label={live.refreshing ? "사용량 읽는 중" : `${usageAt ? `사용량 ${timeOf(usageAt)} 기준` : "사용량 없음"}, 눌러서 새로 읽기`}
            title={unavailable ? `읽지 못한 서비스\n${unavailable}` : "Codex·Antigravity CLI에 사용량을 다시 묻습니다 (o usage와 같음)"}
            className={`ml-auto md:ml-0 disabled:opacity-60 ${BUTTON}`}
          >
            <RefreshCw className={`size-3 ${live.refreshing ? "animate-spin motion-reduce:animate-none" : ""}`} aria-hidden="true" />
            {live.refreshing ? "사용량 읽는 중" : usageAt ? `사용량 ${timeOf(usageAt)} 기준` : "사용량 읽기"}
          </button>
        </>
      ) : (
        <>
          <span className="ml-auto max-md:hidden">{replaying ? "미리보기: 기록을 재생하는 중" : "미리보기: 기록 재생"}</span>
          {state === "full" && (
            <button type="button" onClick={onReplay} className={`ml-auto md:ml-0 ${BUTTON}`}>
              <RotateCcw className="size-3" aria-hidden="true" />이 작업 다시 재생
            </button>
          )}
        </>
      )}
    </footer>
  );
}

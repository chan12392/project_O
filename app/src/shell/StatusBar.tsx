import { RefreshCw, RotateCcw } from "lucide-react";
import { useState } from "react";

import { LIMITS } from "../../../src/core/limits.ts";
import type { RunView } from "../../../src/core/snapshot.ts";
import { timeOf } from "../lib/format.ts";
import type { LiveStatus } from "../data/useLiveRuns.ts";
import type { ScreenState } from "../data/useRun.ts";
import { useBudget } from "../data/useBudget.ts";
import { serviceLimits } from "./serviceLimits.ts";
import { backend, type ResourceLease } from "../platform/backend/index.ts";

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
  const [leases, setLeases] = useState<ResourceLease[] | null>(null);
  const [leaseError, setLeaseError] = useState("");
  const [leaseOpen, setLeaseOpen] = useState(false);
  const [recovering, setRecovering] = useState<string | null>(null);
  const inspect = async () => {
    setLeaseOpen(true);
    setLeaseError("");
    try { setLeases(await backend().resources()); }
    catch (error) { setLeases(null); setLeaseError(String(error)); }
  };
  const recover = async (id: string) => {
    setRecovering(id);
    setLeaseError("");
    try { await backend().recoverResource(id); await inspect(); }
    catch (error) { setLeaseError(String(error)); }
    finally { setRecovering(null); }
  };
  const known = state === "full" || state === "empty";
  const cards = runs.flatMap((r) => r.cards);
  // Live runs span many days; only today's starts count against the daily cap.
  const now = new Date();
  const started = cards.filter((c) => c.startedAt && (!live || sameDay(c.startedAt, now))).length;
  const active = cards.filter((c) => c.state === "running").length;
  const budget = useBudget();
  const limits = serviceLimits(cards, budget?.providers);
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
      <span className="max-sm:hidden" title={limits.title}>{!known ? "한도 확인 전" : limits.text}</span>
      {live ? (
        <>
          <div className="relative">
            <button type="button" className={BUTTON} onClick={() => leaseOpen ? setLeaseOpen(false) : void inspect()} aria-expanded={leaseOpen}>자원 예약 점검</button>
            {leaseOpen && <section aria-label="자원 예약 점검" className="absolute bottom-full left-0 z-50 mb-2 max-h-64 w-80 max-w-[80vw] overflow-auto rounded-lg border bg-popover p-3 text-popover-foreground shadow-lg">
              <p className="mb-2">시작 전 중단과 소유자 종료가 확인된 예약만 복구할 수 있습니다.</p>
              {leaseError && <p role="alert">{leaseError}</p>}
              {leases === null ? !leaseError && <p>예약을 읽는 중</p> : leases.length === 0 ? <p>남아 있는 예약이 없습니다.</p> : <ul className="space-y-2">{leases.map((lease) => <li key={lease.id} className="break-all">{lease.runId} / {lease.cardId} · 실행기 PID {lease.ownerPid ?? "불명"} · 작업 PID {lease.workerPid ?? "불명"}<br />{JSON.stringify(lease.resources)}{lease.spawnIntent === false && <button type="button" disabled={recovering !== null} onClick={() => void recover(lease.id)} className={BUTTON}>{recovering === lease.id ? "확인 중" : "복구 확인"}</button>}</li>)}</ul>}
              <button type="button" onClick={() => void inspect()} className={BUTTON}>다시 확인</button>
            </section>}
          </div>
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

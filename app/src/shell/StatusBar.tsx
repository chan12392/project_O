import { RefreshCw, RotateCcw } from "lucide-react";
import { useCallback, useId, useRef, useState } from "react";

import { LIMITS } from "../../../src/core/limits.ts";
import type { RunView } from "../../../src/core/snapshot.ts";
import { timeOf } from "../lib/format.ts";
import { useDismiss } from "../lib/dismiss.ts";
import type { LiveStatus } from "../data/useLiveRuns.ts";
import type { ScreenState } from "../data/useRun.ts";
import { useBudget } from "../data/useBudget.ts";
import { serviceLimits } from "./serviceLimits.ts";
import { backend, type ResourceLease } from "../platform/backend/index.ts";

const BUTTON = "flex h-5 items-center gap-1 rounded px-1.5 whitespace-nowrap hover:bg-accent hover:text-foreground focus-ring";

function resourceNames(resources: unknown): string {
  if (!Array.isArray(resources)) return "자원 범위를 확인할 수 없습니다";
  if (resources.length === 0) return "범위 확인 불가 · 독점 예약";
  return resources.map((value: unknown) => {
    if (!value || typeof value !== "object") return "자원 정보 불명";
    const { kind, key, mode } = value as Record<string, unknown>;
    if ((kind !== "project-path" && kind !== "external") || typeof key !== "string" || (mode !== "read" && mode !== "write")) return "자원 정보 불명";
    return `${kind === "project-path" ? "파일" : "외부"} ${key} (${mode === "read" ? "읽기" : "쓰기"})`;
  }).join(", ");
}

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
  const [loadingLeases, setLoadingLeases] = useState(false);
  const pending = useRef(false);
  const leasePanel = useRef<HTMLElement>(null);
  const leaseButton = useRef<HTMLButtonElement>(null);
  const leasePanelId = useId();
  const closeLeases = useCallback(() => setLeaseOpen(false), []);
  useDismiss(leasePanel, leaseOpen, closeLeases, leaseButton);
  const inspect = async () => {
    setLeaseOpen(true);
    if (pending.current) return;
    pending.current = true;
    setLoadingLeases(true);
    setLeaseError("");
    try { setLeases(await backend().resources()); }
    catch (error) { setLeases(null); setLeaseError(String(error)); }
    finally { pending.current = false; setLoadingLeases(false); }
  };
  const recover = async (id: string, confirmExited = false) => {
    if (pending.current) return;
    pending.current = true;
    setRecovering(id);
    setLeaseError("");
    try {
      await backend().recoverResource(id, confirmExited);
      setLeases(null);
      setLoadingLeases(true);
      setLeases(await backend().resources());
    }
    catch (error) { setLeaseError(String(error)); }
    finally { pending.current = false; setLoadingLeases(false); setRecovering(null); }
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
            <button ref={leaseButton} type="button" className={BUTTON} onClick={() => leaseOpen ? closeLeases() : void inspect()} aria-expanded={leaseOpen} aria-controls={leasePanelId}>자원 예약 점검</button>
            {leaseOpen && <section ref={leasePanel} id={leasePanelId} aria-label="자원 예약 점검" aria-busy={loadingLeases || recovering !== null} className="absolute bottom-full right-0 z-50 mb-2 max-h-64 w-80 max-w-[80vw] overflow-auto rounded-lg border bg-popover p-3 text-popover-foreground shadow-lg">
              <p className="mb-2">시작 전 또는 작업 종료가 기록된 예약은 실행기 종료 확인 후 복구할 수 있습니다.</p>
              {leaseError && <p role="alert">{leaseError}</p>}
              {loadingLeases && <p role="status">예약을 읽는 중입니다.</p>}
              {!loadingLeases && leases !== null && (leases.length === 0 ? <p>남아 있는 예약이 없습니다.</p> : <ul className="space-y-2">{leases.map((lease) => <li key={lease.id} className="break-words border-b pb-2 last:border-0">
                <p>{lease.runId} / {lease.cardId} · {lease.state === "recoverable" ? "복구 가능" : "실행 중이거나 종료 확인 필요"}</p>
                <p className="break-all">{resourceNames(lease.resources)}</p>
                <p>실행기 PID {lease.ownerPid ?? "불명"} · 작업 PID {lease.workerPid ?? "불명"}</p>
                {lease.state === "recoverable" && <button type="button" disabled={recovering !== null} aria-label={`${lease.runId} ${lease.cardId} 예약 복구`} onClick={() => void recover(lease.id)} className={`disabled:opacity-60 ${BUTTON}`}>{recovering === lease.id ? "복구 중" : "예약 복구"}</button>}
                {lease.state === "unknown" && lease.processesExited && lease.exitConfirmed !== true && <button type="button" disabled={recovering !== null} aria-label={`${lease.runId} ${lease.cardId} 직접 확인 후 복구`} onClick={() => {
                  if (window.confirm("O는 카드의 자식 프로세스가 모두 끝났는지 증명하지 못했습니다. 작업 관리자 등에서 이 카드가 실행한 프로그램이 모두 끝났는지 직접 확인한 경우에만 복구하세요. 복구하시겠습니까?")) void recover(lease.id, true);
                }} className={`disabled:opacity-60 ${BUTTON}`}>{recovering === lease.id ? "복구 중" : "직접 확인 후 복구"}</button>}
              </li>)}</ul>)}
              <button type="button" disabled={loadingLeases || recovering !== null} onClick={() => void inspect()} className={`mt-2 disabled:opacity-60 ${BUTTON}`}>다시 확인</button>
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
